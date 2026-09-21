use crate::worker::{self, Sender};
use crate::{
    timeline::{Timeline, View},
    *,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::{
    net::SocketAddr,
    sync::{Arc, Condvar, Mutex, Weak, mpsc as sync},
    thread,
    time::{Duration, Instant},
};
/// Timestamp domain shared by an engine and its external source inputs.
#[derive(Clone, Copy, Debug)]
pub struct MonotonicClock(Instant);
impl MonotonicClock {
    pub fn now_ns(self) -> u64 {
        self.0.elapsed().as_nanos().min(i64::MAX as u128) as u64
    }
}
#[derive(Clone, Debug)]
pub struct Timing {
    pub acquisition_probe: Duration,
    pub steady_probe: Duration,
    pub heartbeat: Duration,
    pub tracked_publish: Duration,
    pub source_timeout: Duration,
    pub connect_timeout: Duration,
    pub retry_min: Duration,
    pub retry_max: Duration,
}
impl Default for Timing {
    fn default() -> Self {
        Self {
            acquisition_probe: Duration::from_millis(50),
            steady_probe: Duration::from_millis(250),
            heartbeat: Duration::from_secs(1),
            tracked_publish: Duration::from_millis(100),
            source_timeout: Duration::from_millis(500),
            connect_timeout: Duration::from_secs(3),
            retry_min: Duration::from_millis(100),
            retry_max: Duration::from_secs(5),
        }
    }
}
impl Timing {
    fn validate(&self) -> Result<()> {
        if [
            self.acquisition_probe,
            self.steady_probe,
            self.heartbeat,
            self.tracked_publish,
            self.source_timeout,
            self.connect_timeout,
            self.retry_min,
            self.retry_max,
        ]
        .iter()
        .any(|d| d.is_zero() || *d > Duration::from_secs(3600))
            || self.retry_min > self.retry_max
        {
            return Err(Error::Invalid(
                "timing intervals must be in (0, 1 hour], retry_min <= retry_max",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub struct LeaderConfig {
    pub bind: SocketAddr,
    pub name: String,
    pub identity: String,
    pub format: FrameFormat,
    pub position: Position,
    pub rate: Rate,
    pub source_kind: SourceKind,
    pub advertise: bool,
    pub discovery: DiscoveryConfig,
    pub timing: Timing,
    pub max_followers: usize,
}
impl Default for LeaderConfig {
    fn default() -> Self {
        Self {
            bind: "0.0.0.0:0".parse().unwrap(),
            name: "Ethersync".into(),
            identity: uuid::Uuid::new_v4().to_string(),
            format: FrameFormat::default(),
            position: Position::ZERO,
            rate: Rate::PAUSED,
            source_kind: SourceKind::Generated,
            advertise: true,
            discovery: DiscoveryConfig::default(),
            timing: Timing::default(),
            max_followers: 64,
        }
    }
}
#[derive(Clone, Debug)]
pub enum Trust {
    TrustedLan,
    Pinned(String),
}
#[derive(Clone, Debug)]
pub struct FollowerConfig {
    pub address: SocketAddr,
    pub bind: SocketAddr,
    pub trust: Trust,
    pub timing: Timing,
    pub correction: CorrectionPolicy,
    pub fallback_position: Position,
    pub fallback_format: FrameFormat,
    /// Emit bounded, nonblocking clock observation events for capture/replay.
    pub clock_diagnostics: bool,
}
impl FollowerConfig {
    pub fn direct(address: SocketAddr) -> Self {
        Self {
            address,
            bind: if address.is_ipv4() {
                "0.0.0.0:0"
            } else {
                "[::]:0"
            }
            .parse()
            .unwrap(),
            trust: Trust::TrustedLan,
            timing: Timing::default(),
            correction: CorrectionPolicy::default(),
            fallback_position: Position::ZERO,
            fallback_format: FrameFormat::default(),
            clock_diagnostics: false,
        }
    }
    pub fn discovered(leader: &DiscoveredLeader, address: SocketAddr) -> Result<Self> {
        if leader.protocol_version != 1 {
            return Err(Error::Protocol(ethersync_protocol::Error::Version(
                leader.protocol_version,
            )));
        }
        if !leader.addresses.contains(&address) {
            return Err(Error::Invalid("address is not in discovered service"));
        }
        let mut c = Self::direct(address);
        c.trust = Trust::Pinned(leader.fingerprint.clone());
        Ok(c)
    }
}
#[derive(Clone, Debug)]
pub enum Event {
    Connection(ConnectionState),
    Correction(Correction),
    ClockObservation(clock::ClockObservation),
    /// Application publication completion relative to the intended probe deadline.
    /// Does not measure UDP departure or physical network latency.
    ProbeTiming {
        lateness_ns: u64,
    },
    SourceHealth(SourceHealth),
    Error(String),
}
/// One reader owns one preallocated triple buffer. Read calls need exclusive access to this handle.
pub struct TimecodeReader {
    pub(crate) output: triple_buffer::Output<View>,
    pub(crate) clock: MonotonicClock,
    _alive: Arc<()>,
}
impl TimecodeReader {
    /// No allocations, mutexes, or network calls. `local_ns` must use this engine's clock.
    pub fn read_at(&mut self, local_ns: u64) -> Reading {
        self.output.read().evaluate(local_ns)
    }
    /// Predict the timecode that should appear after the output's measured delay.
    /// The delay is in local elapsed time; networking is already accounted for.
    /// No allocations, locking, state advancement, or transport changes.
    pub fn read_for_presentation_at(
        &mut self,
        local_ns: u64,
        compensation_delay: Duration,
    ) -> Result<Reading> {
        let delay = u64::try_from(compensation_delay.as_nanos())
            .map_err(|_| Error::Invalid("presentation delay too large"))?;
        Ok(self
            .output
            .read()
            .evaluate_for_presentation(local_ns, delay)?)
    }
    pub fn read_for_presentation(&mut self, compensation_delay: Duration) -> Result<Reading> {
        self.read_for_presentation_at(self.clock.now_ns(), compensation_delay)
    }
    /// Predict the next frame crossing or scheduled control using this engine’s clock.
    /// Allocation-free; recompute after receiving a newer snapshot.
    pub fn next_boundary_at(&mut self, local_ns: u64) -> Option<Boundary> {
        self.output.read().next_boundary(local_ns)
    }
    pub fn next_boundary(&mut self) -> Option<Boundary> {
        self.next_boundary_at(self.clock.now_ns())
    }
    pub fn read(&mut self) -> Reading {
        self.read_at(self.clock.now_ns())
    }
}
const COMMAND_CAPACITY: usize = 64;
const EVENT_CAPACITY: usize = 64;
pub(crate) const READER_CAPACITY: usize = 64;
type Task = worker::Factory;
/// Independently sampled lifetime worker counters, including connection setup.
#[derive(Clone, Copy, Debug, Default)]
pub struct WorkerTiming {
    pub passes: u64,
    pub max_pass_ns: u64,
    pub max_deadline_lateness_ns: u64,
}

/// Owns one polling worker; counters include startup and connection setup.
pub struct Engine {
    tasks: Sender<Task>,
    stop: Arc<AtomicBool>,
    thread: Mutex<Option<thread::JoinHandle<()>>>,
    stops: Mutex<Vec<Weak<AtomicBool>>>,
    discoveries: Mutex<Vec<mdns_sd::ServiceDaemon>>,
    clock: MonotonicClock,
    signal: Arc<worker::Signal>,
}
impl Engine {
    pub fn new() -> Result<Self> {
        let poll = mio::Poll::new()?;
        let signal = Arc::new(worker::Signal {
            wake: mio::Waker::new(poll.registry(), mio::Token(0))?,
            notified: AtomicBool::new(false),
            passes: Default::default(),
            max_pass_ns: Default::default(),
            max_deadline_lateness_ns: Default::default(),
        });
        let (tasks, rx) = worker::channel(COMMAND_CAPACITY, signal.clone());
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let notify = signal.clone();
        let clock = MonotonicClock(Instant::now());
        let thread = thread::Builder::new()
            .name("ethersync".into())
            .spawn(move || {
                let _ = worker::run(poll, rx, stopped, notify);
            })?;
        Ok(Self {
            tasks,
            signal,
            stop,
            thread: Mutex::new(Some(thread)),
            stops: Mutex::new(Vec::new()),
            discoveries: Mutex::new(Vec::new()),
            clock,
        })
    }
    /// Lifetime scheduling high-water marks; no allocation or worker round trip.
    pub fn worker_timing(&self) -> WorkerTiming {
        WorkerTiming {
            passes: self.signal.passes.load(Ordering::Relaxed),
            max_pass_ns: self.signal.max_pass_ns.load(Ordering::Relaxed),
            max_deadline_lateness_ns: self.signal.max_deadline_lateness_ns.load(Ordering::Relaxed),
        }
    }
    pub fn clock(&self) -> MonotonicClock {
        self.clock
    }
    fn spawn(&self, task: Task) -> Result<()> {
        if self.stop.load(Ordering::Acquire) {
            return Err(Error::Shutdown);
        }
        self.tasks.try_send(task).map_err(queue_error)
    }
    pub fn leader(&self, config: LeaderConfig) -> Result<Leader> {
        config.timing.validate()?;
        if config.max_followers == 0
            || config.max_followers > 1024
            || config.name.is_empty()
            || config.name.len() > 63
            || config.identity.len() > 100
        {
            return Err(Error::Invalid("leader name, identity or follower capacity"));
        }
        let (control, commands, events, completion) = Control::new(self.signal.clone());
        self.register_stop(&control.stop)?;
        let stop = control.stop.clone();
        let clock = self.clock;
        let (tx, rx) = sync::sync_channel(1);
        self.spawn(Box::new(move |registry, token| {
            crate::network::LeaderJob::new(
                config, commands, events, stop, clock, tx, registry, token,
            )
            .map(|job| (Box::new(job) as Box<dyn worker::Job>, completion))
        }))?;
        let info = rx.recv().map_err(|_| Error::Shutdown)??;
        Ok(Leader { control, info })
    }
    pub fn follower(&self, config: FollowerConfig) -> Result<Follower> {
        config.timing.validate()?;
        let p = config.correction;
        if !p.slew_frames_per_second.is_finite()
            || p.slew_frames_per_second <= 0.
            || !p.hard_threshold_frames.is_finite()
            || p.hard_threshold_frames <= 0.
            || p.confirmations == 0
        {
            return Err(Error::Invalid("correction policy"));
        }
        if config.bind.is_ipv4() != config.address.is_ipv4() {
            return Err(Error::Invalid("bind/address families differ"));
        }
        if let Trust::Pinned(ref fingerprint) = config.trust {
            crate::transport::tls::validate_pin(fingerprint)
                .map_err(|e| Error::Transport(e.to_string()))?;
        }
        let (control, commands, events, completion) = Control::new(self.signal.clone());
        self.register_stop(&control.stop)?;
        let stop = control.stop.clone();
        let clock = self.clock;
        self.spawn(Box::new(move |registry, token| {
            crate::network::FollowerJob::new(config, commands, events, stop, clock, registry, token)
                .map(|job| (Box::new(job) as Box<dyn worker::Job>, completion))
        }))?;
        Ok(Follower { control })
    }
    pub fn discovery(&self, config: DiscoveryConfig) -> Result<Discovery> {
        let mut owned = self.discoveries.lock().map_err(|_| Error::Shutdown)?;
        if self.stop.load(Ordering::Acquire) {
            return Err(Error::Shutdown);
        }
        let d = Discovery::new(config)?;
        owned.push(d.daemon.clone());
        Ok(d)
    }
    fn register_stop(&self, stop: &Arc<AtomicBool>) -> Result<()> {
        let mut stops = self.stops.lock().map_err(|_| Error::Shutdown)?;
        if self.stop.load(Ordering::Acquire) {
            return Err(Error::Shutdown);
        }
        stops.retain(|s| s.upgrade().is_some());
        stops.push(Arc::downgrade(stop));
        Ok(())
    }
    pub fn shutdown(&self) -> Result<()> {
        {
            let stops = self.stops.lock().map_err(|_| Error::Shutdown)?;
            self.stop.store(true, Ordering::Release);
            self.signal.notify();
            for stop in stops.iter().filter_map(|s| s.upgrade()) {
                stop.store(true, Ordering::Release);
            }
        }
        for d in self
            .discoveries
            .lock()
            .map_err(|_| Error::Shutdown)?
            .drain(..)
        {
            if let Ok(rx) = d.shutdown() {
                let _ = rx.recv_timeout(Duration::from_secs(1));
            }
        }
        if let Some(t) = self.thread.lock().map_err(|_| Error::Shutdown)?.take() {
            t.join().map_err(|_| Error::Shutdown)?;
        }
        Ok(())
    }
}
impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}
fn queue_error<T>(e: sync::TrySendError<T>) -> Error {
    match e {
        sync::TrySendError::Full(_) => Error::QueueFull,
        sync::TrySendError::Disconnected(_) => Error::Shutdown,
    }
}
#[derive(Clone, Debug)]
pub struct LeaderInfo {
    pub address: SocketAddr,
    pub identity: String,
    pub session: [u8; 16],
    pub fingerprint: String,
}
pub struct Leader {
    control: Control,
    pub(crate) info: LeaderInfo,
}
impl Leader {
    pub fn info(&self) -> &LeaderInfo {
        &self.info
    }
    pub fn reader(&self) -> Result<TimecodeReader> {
        self.control.reader()
    }
    pub fn try_event(&self) -> Option<Event> {
        self.control.event()
    }
    pub fn play(&self) -> Result<()> {
        self.control.change(Change::Rate(Rate::NORMAL), None)
    }
    pub fn pause(&self) -> Result<()> {
        self.control.change(Change::Rate(Rate::PAUSED), None)
    }
    pub fn seek(&self, position: Position) -> Result<()> {
        self.control.change(Change::Seek(position), None)
    }
    pub fn speed(&self, rate: Rate) -> Result<()> {
        self.control.change(Change::Rate(rate), None)
    }
    /// Atomically set position and speed now, or at an explicit engine-clock timestamp.
    /// Future timestamps schedule a control; past timestamps apply immediately and
    /// extrapolate the supplied position to the present without adding queue latency.
    pub fn set_transport(
        &self,
        position: Position,
        rate: Rate,
        effective_ns: Option<u64>,
    ) -> Result<()> {
        self.control
            .change(Change::Set(position, rate), effective_ns)
    }
    pub fn track(&self, sample: SourceSample) -> Result<()> {
        let (tx, rx) = sync::sync_channel(1);
        self.control
            .commands
            .try_send(Command::Sample(sample, tx))
            .map_err(queue_error)?;
        rx.recv().map_err(|_| Error::Shutdown)?
    }
    pub fn shutdown(&self) -> Result<()> {
        self.control.shutdown()
    }
}
pub struct Follower {
    control: Control,
}
impl Follower {
    pub fn reader(&self) -> Result<TimecodeReader> {
        self.control.reader()
    }
    pub fn try_event(&self) -> Option<Event> {
        self.control.event()
    }
    /// Force a fresh connection to the same endpoint; timeline remains in holdover.
    pub fn reconnect(&self) -> Result<()> {
        self.control
            .commands
            .try_send(Command::Reconnect)
            .map_err(queue_error)
    }
    pub fn shutdown(&self) -> Result<()> {
        self.control.shutdown()
    }
}
pub(crate) struct Completion(Arc<(Mutex<bool>, Condvar)>);
impl Drop for Completion {
    fn drop(&mut self) {
        if let Ok(mut done) = self.0.0.lock() {
            *done = true;
            self.0.1.notify_all();
        }
    }
}
struct Control {
    commands: Sender<Command>,
    events: Mutex<sync::Receiver<Event>>,
    stop: Arc<AtomicBool>,
    completed: Arc<(Mutex<bool>, Condvar)>,
    signal: Arc<worker::Signal>,
}
impl Control {
    fn new(
        signal: Arc<worker::Signal>,
    ) -> (
        Self,
        sync::Receiver<Command>,
        sync::SyncSender<Event>,
        Completion,
    ) {
        let (commands, rx) = worker::channel(COMMAND_CAPACITY, signal.clone());
        let (events, erx) = sync::sync_channel(EVENT_CAPACITY);
        let stop = AtomicBool::new(false);
        let completed = Arc::new((Mutex::new(false), Condvar::new()));
        (
            Self {
                commands,
                signal,
                events: Mutex::new(erx),
                stop: Arc::new(stop),
                completed: completed.clone(),
            },
            rx,
            events,
            Completion(completed),
        )
    }
    fn reader(&self) -> Result<TimecodeReader> {
        let (tx, rx) = sync::sync_channel(1);
        self.commands
            .try_send(Command::Reader(tx))
            .map_err(queue_error)?;
        rx.recv().map_err(|_| Error::Shutdown)?
    }
    fn event(&self) -> Option<Event> {
        self.events.lock().ok()?.try_recv().ok()
    }
    fn change(&self, change: Change, effective: Option<u64>) -> Result<()> {
        let (tx, rx) = sync::sync_channel(1);
        self.commands
            .try_send(Command::Change(change, effective, tx))
            .map_err(queue_error)?;
        rx.recv().map_err(|_| Error::Shutdown)?
    }
    fn shutdown(&self) -> Result<()> {
        self.stop.store(true, Ordering::Release);
        self.signal.notify();
        let done = self.completed.0.lock().map_err(|_| Error::Shutdown)?;
        let (done, _) = self
            .completed
            .1
            .wait_timeout_while(done, Duration::from_secs(5), |done| !*done)
            .map_err(|_| Error::Shutdown)?;
        if !*done {
            return Err(Error::Transport(
                "shutdown did not finish within five seconds".into(),
            ));
        }
        Ok(())
    }
}
impl Drop for Control {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.signal.notify();
    }
}
pub use ethersync_protocol::SourceSample;
pub(crate) enum Change {
    Rate(Rate),
    Seek(Position),
    Set(Position, Rate),
}
pub(crate) enum Command {
    Reader(sync::SyncSender<Result<TimecodeReader>>),
    Change(Change, Option<u64>, sync::SyncSender<Result<()>>),
    Sample(SourceSample, sync::SyncSender<Result<()>>),
    Reconnect,
}
pub(crate) struct Readers {
    inputs: Vec<(triple_buffer::Input<View>, Weak<()>)>,
    clock: MonotonicClock,
}
impl Readers {
    pub fn new(clock: MonotonicClock) -> Self {
        Self {
            inputs: Vec::new(),
            clock,
        }
    }
    pub fn add(&mut self, view: View, reply: sync::SyncSender<Result<TimecodeReader>>) {
        // Reclaim abandoned reader slots before checking the live-reader bound.
        self.inputs.retain(|(_, alive)| alive.strong_count() > 0);
        if self.inputs.len() >= READER_CAPACITY {
            let _ = reply.send(Err(Error::ReaderLimit));
            return;
        }
        let (input, output) = triple_buffer::triple_buffer(&view);
        let alive = Arc::new(());
        self.inputs.push((input, Arc::downgrade(&alive)));
        let _ = reply.send(Ok(TimecodeReader {
            output,
            clock: self.clock,
            _alive: alive,
        }));
    }
    pub fn publish(&mut self, view: View) {
        self.inputs.retain(|(_, alive)| alive.strong_count() > 0);
        for (input, _) in &mut self.inputs {
            input.write(view);
        }
    }
}
pub(crate) fn emit(events: &sync::SyncSender<Event>, event: Event) {
    let _ = events.try_send(event);
}
pub(crate) fn fallback(c: &FollowerConfig) -> Timeline {
    Timeline {
        format: c.fallback_format,
        anchor: crate::timeline::Anchor {
            position: c.fallback_position,
            ..Default::default()
        },
        ..Default::default()
    }
}
