//! MoQ adapter for owner-driven polling. Registration does not execute work.
use crate::transport::{moq, web::Session};
use moq::kio;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{Arc, Mutex, Weak},
    task::{Context, Poll},
    time::Instant,
};

#[derive(Default)]
struct TimerState {
    at: Option<Instant>,
    waiters: kio::WaiterList,
}
#[derive(Clone, Default)]
pub struct Timers(Arc<Mutex<Vec<Weak<Mutex<TimerState>>>>>);
pub struct Timer(Arc<Mutex<TimerState>>);
impl Timer {
    pub fn set(&mut self, at: Option<Instant>) {
        let mut s = self.0.lock().unwrap();
        if s.at == at {
            return;
        }
        s.at = at;
        let mut waiters = std::mem::take(&mut s.waiters);
        drop(s);
        // Rescheduling must interrupt an owner's wait on the old deadline.
        // Wakers may reenter: never call them while holding the timer lock.
        waiters.wake();
    }
    pub fn poll(&mut self, waiter: &kio::Waiter) -> Poll<()> {
        let mut s = self.0.lock().unwrap();
        if s.at.is_some_and(|at| at <= Instant::now()) {
            Poll::Ready(())
        } else {
            waiter.register(&mut s.waiters);
            Poll::Pending
        }
    }
}
impl Timers {
    pub fn timer(&self) -> Timer {
        let s = Arc::new(Mutex::new(TimerState::default()));
        self.0.lock().unwrap().push(Arc::downgrade(&s));
        Timer(s)
    }
}
impl Timers {
    pub fn advance(&self, now: Instant) -> Option<Instant> {
        let mut next = None;
        let mut wakes = Vec::new();
        self.0.lock().unwrap().retain(|weak| {
            let Some(s) = weak.upgrade() else {
                return false;
            };
            let mut s = s.lock().unwrap();
            if let Some(at) = s.at {
                if at <= now {
                    wakes.push(std::mem::take(&mut s.waiters));
                } else {
                    next = Some(next.map_or(at, |old: Instant| old.min(at)));
                }
            }
            true
        });
        for mut waiters in wakes {
            waiters.wake();
        }
        next
    }
}
#[derive(Clone, Default)]
pub struct Runtime {
    timers: Timers,
    ready: Rc<Cell<bool>>,
    deadline: Rc<Cell<Option<Instant>>>,
    machines: Rc<RefCell<Vec<moq::Driver<Session>>>>,
}
impl Runtime {
    pub fn spawn(&self, driver: moq::Driver<Session>) {
        self.machines.borrow_mut().push(driver);
        self.ready.set(true);
    }
    pub fn deadline(&self) -> Option<Instant> {
        self.deadline.get()
    }
    pub fn needs_pass(&self) -> bool {
        self.ready.get()
    }
    pub fn timers(&self) -> Timers {
        self.timers.clone()
    }
    pub fn step(&self, cx: &mut Context<'_>, park: &mut kio::Park) {
        self.ready.set(false);
        let mut machines = std::mem::take(&mut *self.machines.borrow_mut());
        let waiter = park.hold(cx);
        let now = Instant::now();
        let mut deadline = None;
        machines.retain_mut(|m| match m.poll(now, waiter) {
            Ok(at) => {
                deadline = deadline.into_iter().chain(at).min();
                true
            }
            Err(_) => false,
        });
        self.deadline.set(deadline);
        let mut pending = self.machines.borrow_mut();
        machines.append(&mut *pending);
        *pending = machines;
    }
    pub fn clear(&self) {
        self.machines.borrow_mut().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::atomic::{AtomicUsize, Ordering},
        task::{Wake, Waker},
        time::Duration,
    };

    #[derive(Default)]
    struct Counter(AtomicUsize);
    impl Wake for Counter {
        fn wake(self: Arc<Self>) {
            self.wake_by_ref();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
    #[test]
    fn reschedule_cancel_expire_and_drop() {
        let timers = Timers::default();
        let mut timer = timers.timer();
        let counter = Arc::new(Counter::default());
        let waker = Waker::from(counter.clone());
        let cx = Context::from_waker(&waker);
        let mut park = kio::Park::default();
        let now = Instant::now();
        let late = now + Duration::from_secs(60);
        let early = now + Duration::from_secs(30);
        timer.set(Some(late));
        assert!(timer.poll(park.hold(&cx)).is_pending());
        assert_eq!(timers.advance(now), Some(late));
        timer.set(Some(early));
        assert_eq!(counter.0.load(Ordering::Relaxed), 1);
        assert_eq!(timers.advance(now), Some(early));
        assert!(timer.poll(park.hold(&cx)).is_pending());
        timer.set(None);
        assert_eq!(counter.0.load(Ordering::Relaxed), 2);
        assert_eq!(timers.advance(now), None);
        assert!(timer.poll(park.hold(&cx)).is_pending());
        timer.set(Some(now));
        assert!(timer.poll(park.hold(&cx)).is_ready());
        timer.set(Some(early));
        assert!(timer.poll(park.hold(&cx)).is_pending());
        let before = counter.0.load(Ordering::Relaxed);
        assert_eq!(timers.advance(early), None);
        assert_eq!(counter.0.load(Ordering::Relaxed), before + 1);
        timers.advance(early);
        assert_eq!(counter.0.load(Ordering::Relaxed), before + 1);
        drop(timer);
        timers.advance(now);
        assert!(timers.0.lock().unwrap().is_empty());
    }

    struct Reentrant(Timers, Arc<Mutex<TimerState>>);
    impl Wake for Reentrant {
        fn wake(self: Arc<Self>) {
            self.wake_by_ref();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            assert!(self.0.0.try_lock().is_ok(), "registry locked during wake");
            assert!(self.1.try_lock().is_ok(), "timer locked during wake");
        }
    }
    #[test]
    fn timer_wakers_can_reenter_the_registry_and_timer() {
        let timers = Timers::default();
        let mut timer = timers.timer();
        let waker = Waker::from(Arc::new(Reentrant(timers.clone(), timer.0.clone())));
        let cx = Context::from_waker(&waker);
        let mut park = kio::Park::default();
        let future = Instant::now() + Duration::from_secs(60);
        timer.set(Some(future));
        assert!(timer.poll(park.hold(&cx)).is_pending());
        timer.set(None);
        timer.set(Some(future));
        assert!(timer.poll(park.hold(&cx)).is_pending());
        timers.advance(future);
    }
}
