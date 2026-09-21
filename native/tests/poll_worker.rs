use libethersync::{Engine, FollowerConfig, LeaderConfig, Rate, SyncState};
use std::time::{Duration, Instant};

#[test]
fn fifteen_followers_share_one_worker_and_track_the_same_instant() {
    let engine = Engine::new().unwrap();
    let leader = engine
        .leader(LeaderConfig {
            bind: "127.0.0.1:0".parse().unwrap(),
            advertise: false,
            rate: Rate::NORMAL,
            ..Default::default()
        })
        .unwrap();
    let mut followers = Vec::new();
    for _ in 0..15 {
        let f = engine
            .follower(FollowerConfig::direct(leader.info().address))
            .unwrap();
        let r = f.reader().unwrap();
        followers.push((f, r));
    }
    let mut source = leader.reader().unwrap();
    let start = Instant::now();
    loop {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "all followers must acquire"
        );
        if followers
            .iter_mut()
            .all(|(_, r)| r.read().status.synchronization == SyncState::Synchronized)
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let mut errors = Vec::new();
    for _ in 0..100 {
        let now = engine.clock().now_ns();
        let expected = source.read_at(now).position.as_frames();
        for (_, r) in &mut followers {
            errors.push((r.read_at(now).position.as_frames() - expected).abs() / 30. * 1000.);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    errors.sort_by(f64::total_cmp);
    let p95 = errors[errors.len() * 95 / 100];
    eprintln!(
        "15 followers acquisition+measurement {:?}; p95 {p95:.3} ms",
        start.elapsed()
    );
    assert!(p95 <= 2., "same-worker clock error {p95} ms");
    engine.shutdown().unwrap();
}

#[test]
fn probe_deadline_tails_under_control_and_reconnect_load() {
    probe_load(false);
}

#[test]
#[ignore = "requires local multicast discovery"]
fn probe_deadline_tails_with_discovery() {
    probe_load(true);
}

fn probe_load(discovery: bool) {
    use libethersync::{Event, Position};
    let engine = Engine::new().unwrap();
    let _discovery = discovery.then(|| engine.discovery(Default::default()).unwrap());
    let leader = engine
        .leader(LeaderConfig {
            bind: "127.0.0.1:0".parse().unwrap(),
            advertise: discovery,
            rate: Rate::NORMAL,
            ..Default::default()
        })
        .unwrap();
    let followers: Vec<_> = (0..15)
        .map(|_| {
            let mut config = FollowerConfig::direct(leader.info().address);
            config.clock_diagnostics = true;
            config.timing.steady_probe = Duration::from_millis(50);
            engine.follower(config).unwrap()
        })
        .collect();
    let mut tails = vec![Vec::new(); 15];
    for turn in 0..300 {
        // Immediate controls publish new snapshots while probes are due.
        leader.seek(Position::from_frames(turn)).unwrap();
        if turn % 50 == 0 {
            followers[(turn as usize / 50) % 15].reconnect().unwrap();
        }
        for (index, follower) in followers.iter().enumerate() {
            while let Some(event) = follower.try_event() {
                if let Event::ProbeTiming { lateness_ns } = event {
                    tails[index].push(lateness_ns);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        tails.iter().all(|samples| samples.len() >= 20),
        "every follower must keep probing"
    );
    let mut samples: Vec<_> = tails.into_iter().flatten().collect();
    samples.sort_unstable();
    let ms = |n: u64| n as f64 / 1e6;
    eprintln!(
        "probe publication lateness under load: n={} p95={:.3}ms p99={:.3}ms max={:.3}ms; worker {:?}",
        samples.len(),
        ms(samples[samples.len() * 95 / 100]),
        ms(samples[samples.len() * 99 / 100]),
        ms(*samples.last().unwrap()),
        engine.worker_timing()
    );
    // A starvation guard, not a hard-real-time promise for a shared CI host.
    assert!(*samples.last().unwrap() < 500_000_000);
    engine.shutdown().unwrap();
}

#[test]
fn idle_worker_sleeps_and_command_wakeup_publishes_before_ack() {
    let engine = Engine::new().unwrap();
    let leader = engine
        .leader(LeaderConfig {
            bind: "127.0.0.1:0".parse().unwrap(),
            advertise: false,
            ..Default::default()
        })
        .unwrap();
    let mut reader = leader.reader().unwrap();
    std::thread::sleep(Duration::from_millis(30));
    let before = engine.worker_timing().passes;
    std::thread::sleep(Duration::from_millis(100));
    assert!(
        engine.worker_timing().passes - before <= 4,
        "idle worker is spinning"
    );
    leader
        .seek(libethersync::Position::from_frames(42))
        .unwrap();
    assert_eq!(
        reader.read().position,
        libethersync::Position::from_frames(42)
    );
    engine.shutdown().unwrap();
}
