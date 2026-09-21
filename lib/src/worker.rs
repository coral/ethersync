//! One readiness loop per engine. No async executor or worker pool.
use crate::{Result, api::Completion};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    task::{Context, Wake, Waker},
    time::Instant,
};
pub(crate) trait Job {
    /// False removes and drops this job.
    fn step(&mut self, cx: &mut Context<'_>) -> bool;
    fn deadline(&self) -> Option<Instant>;
    fn ready(&self) -> bool;
}
pub(crate) type Factory =
    Box<dyn FnOnce(&mio::Registry, mio::Token) -> Option<(Box<dyn Job>, Completion)> + Send>;
pub(crate) struct Signal {
    pub wake: mio::Waker,
    pub notified: AtomicBool,
    pub passes: AtomicU64,
    pub max_pass_ns: AtomicU64,
    pub max_deadline_lateness_ns: AtomicU64,
}
impl Signal {
    pub fn notify(&self) {
        self.notified.store(true, Ordering::Release);
        let _ = self.wake.wake();
    }
}
impl Wake for Signal {
    fn wake(self: Arc<Self>) {
        self.notify();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.notify();
    }
}
pub(crate) struct Sender<T> {
    tx: mpsc::SyncSender<T>,
    signal: Arc<Signal>,
}
impl<T> Sender<T> {
    pub fn try_send(&self, value: T) -> std::result::Result<(), mpsc::TrySendError<T>> {
        self.tx.try_send(value)?;
        self.signal.notify();
        Ok(())
    }
}
pub(crate) fn channel<T>(capacity: usize, signal: Arc<Signal>) -> (Sender<T>, mpsc::Receiver<T>) {
    let (tx, rx) = mpsc::sync_channel(capacity);
    (Sender { tx, signal }, rx)
}
pub(crate) fn run(
    mut poll: mio::Poll,
    rx: mpsc::Receiver<Factory>,
    stop: Arc<AtomicBool>,
    signal: Arc<Signal>,
) -> Result<()> {
    let waker = Waker::from(signal.clone());
    let mut cx = Context::from_waker(&waker);
    let mut events = mio::Events::with_capacity(128);
    let mut jobs: Vec<(Box<dyn Job>, Completion)> = Vec::new();
    let mut token = 1;
    let mut scheduled_deadline: Option<Instant> = None;
    loop {
        let began = Instant::now();
        if let Some(at) = scheduled_deadline {
            signal.max_deadline_lateness_ns.fetch_max(
                began
                    .saturating_duration_since(at)
                    .as_nanos()
                    .min(u64::MAX as u128) as u64,
                Ordering::Relaxed,
            );
        }
        signal.notified.store(false, Ordering::Release);
        let mut factory_more = false;
        for operation in 0..64 {
            let Ok(factory) = rx.try_recv() else {
                break;
            };
            factory_more = operation == 63;
            if let Some(job) = factory(poll.registry(), mio::Token(token)) {
                jobs.push(job);
            }
            token += 1;
        }
        if jobs.len() > 1 {
            jobs.rotate_left(1);
        }
        jobs.retain_mut(|(job, _)| job.step(&mut cx));
        if stop.load(Ordering::Acquire) {
            break;
        }
        let deadline = jobs.iter().filter_map(|(job, _)| job.deadline()).min();
        let immediate = factory_more
            || signal.notified.load(Ordering::Acquire)
            || jobs.iter().any(|(job, _)| job.ready());
        let timeout = if immediate {
            Some(std::time::Duration::ZERO)
        } else {
            deadline.map(|at| at.saturating_duration_since(Instant::now()))
        };
        scheduled_deadline = deadline;
        signal.passes.fetch_add(1, Ordering::Relaxed);
        signal.max_pass_ns.fetch_max(
            began.elapsed().as_nanos().min(u64::MAX as u128) as u64,
            Ordering::Relaxed,
        );
        match poll.poll(&mut events, timeout) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
