use libethersync::*;
use std::{
    thread,
    time::{Duration, Instant},
};
fn config() -> LeaderConfig {
    LeaderConfig {
        bind: "127.0.0.1:0".parse().unwrap(),
        advertise: false,
        ..Default::default()
    }
}
fn wait(
    reader: &mut TimecodeReader,
    follower: &Follower,
    predicate: impl Fn(Reading) -> bool,
) -> Reading {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let r = reader.read();
        if predicate(r) {
            return r;
        }
        if Instant::now() > deadline {
            while let Some(e) = follower.try_event() {
                eprintln!("{e:?}");
            }
            panic!("timed out: {r:?}");
        }
        thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn quic_multiple_followers_controls_holdover_and_reconnect() {
    let leader_engine = Engine::new().unwrap();
    let leader = leader_engine.leader(config()).unwrap();
    leader.play().unwrap();
    let engine = Engine::new().unwrap();
    let mut followers = Vec::new();
    for _ in 0..3 {
        let mut c = FollowerConfig::direct(leader.info().address);
        c.trust = Trust::Pinned(leader.info().fingerprint.clone());
        let f = engine.follower(c).unwrap();
        let mut r = f.reader().unwrap();
        wait(&mut r, &f, |r| {
            r.status.synchronization == SyncState::Synchronized
        });
        followers.push((f, r));
    }
    leader
        .set_transport(Position::from_frames(900), Rate::new(-1, 2).unwrap(), None)
        .unwrap();
    for (f, r) in &mut followers {
        let v = wait(r, f, |r| r.rate == Rate::new(-1, 2).unwrap());
        assert!((v.position.as_frames() - 900.).abs() < 30.);
    }
    let due = leader_engine.clock().now_ns() + 300_000_000;
    leader
        .set_transport(Position::from_frames(5000), Rate::PAUSED, Some(due))
        .unwrap();
    for (f, r) in &mut followers {
        wait(r, f, |r| {
            r.position == Position::from_frames(5000) && r.rate == Rate::PAUSED
        });
    }
    followers[0].0.reconnect().unwrap();
    let (f, r) = &mut followers[0];
    wait(r, f, |r| {
        r.status.synchronization == SyncState::Synchronized
    });
    leader_engine.shutdown().unwrap();
    for (f, r) in &mut followers {
        let v = wait(r, f, |r| r.status.synchronization == SyncState::Holdover);
        assert_eq!(v.position, Position::from_frames(5000));
    }
    let before = followers[0].1.read();
    thread::sleep(Duration::from_millis(100));
    let after = followers[0].1.read();
    assert!(after.status.uncertainty_ns > before.status.uncertainty_ns);
    engine.shutdown().unwrap();
}
#[test]
fn bad_pin_and_fallback() {
    let e = Engine::new().unwrap();
    let l = e.leader(config()).unwrap();
    let mut c = FollowerConfig::direct(l.info().address);
    c.trust = Trust::Pinned("00".repeat(32));
    let f = e.follower(c).unwrap();
    let mut r = f.reader().unwrap();
    thread::sleep(Duration::from_millis(300));
    assert_eq!(r.read().status.synchronization, SyncState::Uninitialized);
    assert_eq!(r.read().label().to_string(), "00:00:00:00");
    e.shutdown().unwrap();
}
#[test]
fn repeated_start_shutdown_and_reader_limit() {
    for _ in 0..3 {
        let e = Engine::new().unwrap();
        let l = e.leader(config()).unwrap();
        let mut readers = Vec::new();
        for _ in 0..64 {
            readers.push(l.reader().unwrap());
        }
        assert!(matches!(l.reader(), Err(Error::ReaderLimit)));
        // An undrained event queue cannot block controls or synchronization.
        for i in 0..100 {
            l.seek(Position::from_frames(i)).unwrap();
        }
        e.shutdown().unwrap();
        assert!(l.play().is_err());
    }
}

#[test]
fn session_restart_same_address_and_running_holdover() {
    let leader_engine = Engine::new().unwrap();
    let leader = leader_engine.leader(config()).unwrap();
    leader.speed(Rate::new(-1, 1).unwrap()).unwrap();
    let address = leader.info().address;
    let follower_engine = Engine::new().unwrap();
    let f = follower_engine
        .follower(FollowerConfig::direct(address))
        .unwrap();
    let mut r = f.reader().unwrap();
    wait(&mut r, &f, |r| {
        r.status.synchronization == SyncState::Synchronized
    });
    leader.shutdown().unwrap();
    wait(&mut r, &f, |r| {
        r.status.synchronization == SyncState::Holdover
    });
    let a = r.read();
    thread::sleep(Duration::from_millis(100));
    assert!(r.read().position < a.position);
    let next = leader_engine
        .leader(LeaderConfig {
            bind: address,
            position: Position::from_frames(10000),
            ..config()
        })
        .unwrap();
    assert_ne!(next.info().session, leader.info().session);
    wait(&mut r, &f, |r| {
        r.status.synchronization == SyncState::Synchronized
            && r.position == Position::from_frames(10000)
    });
    f.shutdown().unwrap();
    assert_eq!(r.read().status.connection, ConnectionState::Shutdown);
    follower_engine.shutdown().unwrap();
    leader_engine.shutdown().unwrap();
}
#[test]
fn tracked_source_health_survives_network_and_input_loss() {
    let e = Engine::new().unwrap();
    let l = e
        .leader(LeaderConfig {
            source_kind: SourceKind::Tracked,
            ..config()
        })
        .unwrap();
    let f = e
        .follower(FollowerConfig::direct(l.info().address))
        .unwrap();
    let mut r = f.reader().unwrap();
    for i in 0..15 {
        let now = e.clock().now_ns();
        l.track(SourceSample {
            timestamp_ns: now,
            position: Position::from_frames(i * 3),
            rate_hint: Some(Rate::NORMAL),
            discontinuity: i == 0,
        })
        .unwrap();
        thread::sleep(Duration::from_millis(100));
    }
    wait(&mut r, &f, |r| {
        r.status.synchronization == SyncState::Synchronized
            && r.status.source_health == SourceHealth::Healthy
    });
    let degraded = wait(&mut r, &f, |r| {
        r.status.source_health == SourceHealth::Degraded
    });
    assert_eq!(degraded.status.source_kind, SourceKind::Tracked);
    assert_eq!(degraded.rate, Rate::NORMAL);
    l.track(SourceSample {
        timestamp_ns: e.clock().now_ns(),
        position: Position::from_frames(900),
        rate_hint: Some(Rate::PAUSED),
        discontinuity: true,
    })
    .unwrap();
    wait(&mut r, &f, |r| {
        r.position == Position::from_frames(900) && r.status.source_health == SourceHealth::Healthy
    });
    e.shutdown().unwrap();
}
#[test]
fn ipv6_loopback() {
    let e = Engine::new().unwrap();
    let l = e
        .leader(LeaderConfig {
            bind: "[::1]:0".parse().unwrap(),
            ..config()
        })
        .unwrap();
    let f = e
        .follower(FollowerConfig::direct(l.info().address))
        .unwrap();
    let mut r = f.reader().unwrap();
    wait(&mut r, &f, |r| {
        r.status.synchronization == SyncState::Synchronized
    });
    e.shutdown().unwrap();
}
#[test]
fn undrained_events_do_not_block_synchronization() {
    let e = Engine::new().unwrap();
    let l = e.leader(config()).unwrap();
    let mut c = FollowerConfig::direct(l.info().address);
    c.clock_diagnostics = true;
    let f = e.follower(c).unwrap();
    let mut r = f.reader().unwrap();
    wait(&mut r, &f, |r| {
        r.status.synchronization == SyncState::Synchronized
    });
    for i in 0..150 {
        l.seek(Position::from_frames(i)).unwrap();
        thread::sleep(Duration::from_millis(5));
    }
    wait(&mut r, &f, |r| r.position == Position::from_frames(149));
    e.shutdown().unwrap();
}

#[test]
fn cancellation_during_connection_attempt() {
    let engine = Engine::new().unwrap();
    let follower = engine
        .follower(FollowerConfig::direct("127.0.0.1:9".parse().unwrap()))
        .unwrap();
    let mut reader = follower.reader().unwrap();
    let began = Instant::now();
    follower.shutdown().unwrap();
    assert!(began.elapsed() < Duration::from_secs(1));
    assert_eq!(reader.read().status.connection, ConnectionState::Shutdown);
    assert_eq!(
        reader.read().status.synchronization,
        SyncState::Uninitialized
    );
    engine.shutdown().unwrap();
}

#[test]
fn same_instant_readings_with_independent_engine_epochs() {
    let leader_engine = Engine::new().unwrap();
    let leader = leader_engine.leader(config()).unwrap();
    leader.play().unwrap();
    // Match separately launched processes: their clock epochs differ even on one host.
    thread::sleep(Duration::from_millis(250));
    let follower_engine = Engine::new().unwrap();
    let follower = follower_engine
        .follower(FollowerConfig::direct(leader.info().address))
        .unwrap();
    let mut leader_reader = leader.reader().unwrap();
    let mut follower_reader = follower.reader().unwrap();
    wait(&mut follower_reader, &follower, |r| {
        r.status.synchronization == SyncState::Synchronized
    });
    // Independently measure epoch separation, not using the estimator being tested.
    let (span, epoch_delta) = (0..64)
        .map(|_| {
            let before = leader_engine.clock().now_ns();
            let follower_time = follower_engine.clock().now_ns();
            let after = leader_engine.clock().now_ns();
            (
                after - before,
                (before as i128 + after as i128) / 2 - follower_time as i128,
            )
        })
        .min_by_key(|(span, _)| *span)
        .unwrap();
    assert!(span < 100_000, "clock calibration was preempted");
    for rate in [Rate::NORMAL, Rate::new(-1, 1).unwrap()] {
        let expected_discontinuity = leader_reader.read().discontinuity + 1;
        leader.speed(rate).unwrap();
        wait(&mut follower_reader, &follower, |r| {
            r.rate == rate && r.discontinuity == expected_discontinuity
        });
        let mut errors = Vec::new();
        for _ in 0..600 {
            let local = leader_engine.clock().now_ns();
            let remote_local = (local as i128 - epoch_delta) as u64;
            let a = leader_reader.read_at(local);
            let b = follower_reader.read_at(remote_local);
            let error_ms =
                (b.position.fixed() - a.position.fixed()) as f64 / 4294967296.0 / a.format.fps()
                    * 1000.0;
            errors.push(error_ms.abs());
            thread::sleep(Duration::from_millis(5));
        }
        errors.sort_by(f64::total_cmp);
        let p95 = errors[errors.len() * 95 / 100];
        eprintln!(
            "same-instant rate={:+.1}x epoch_delta={:.3}ms calibration_span={}ns p95={:.3}ms max={:.3}ms",
            rate.as_f64(),
            epoch_delta as f64 / 1e6,
            span,
            p95,
            errors.last().unwrap()
        );
        assert!(p95 < 2.0, "same-instant timecode error {p95:.3}ms");
    }
    follower_engine.shutdown().unwrap();
    leader_engine.shutdown().unwrap();
}

#[test]
fn clock_diagnostics_preserve_receipt_timestamps_and_deadlines() {
    let leader_engine = Engine::new().unwrap();
    let leader = leader_engine.leader(config()).unwrap();
    leader.play().unwrap();
    let engine = Engine::new().unwrap();
    let mut c = FollowerConfig::direct(leader.info().address);
    c.clock_diagnostics = true;
    let follower = engine.follower(c).unwrap();
    let mut reader = follower.reader().unwrap();
    let r = wait(&mut reader, &follower, |r| {
        r.status.synchronization == SyncState::Synchronized
    });
    let evidence = r.status.offset_evidence.unwrap();
    assert!(evidence.consistent());
    let now = engine.clock().now_ns();
    let boundary = reader.next_boundary_at(now).unwrap();
    assert!(boundary.local_deadline_ns > now);
    assert_eq!(boundary.kind, BoundaryKind::Frame);
    let mut count = 0;
    while let Some(e) = follower.try_event() {
        if let Event::ClockObservation(o) = e {
            assert!(o.exchange.t4 >= o.exchange.t1);
            assert!(o.publication_ns.is_some());
            assert!(o.exchange.t4 <= engine.clock().now_ns());
            count += 1;
        }
    }
    assert!(count >= 8);
    engine.shutdown().unwrap();
    leader_engine.shutdown().unwrap();
}
