use std::time::{Duration, Instant};
use tidkod::*;

fn wait_for(reader: &mut TimecodeReader, predicate: impl Fn(Reading) -> bool) -> Reading {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let reading = reader.read();
        if predicate(reading) {
            return reading;
        }
        assert!(Instant::now() < deadline, "timed out: {reading:?}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn recording_parts_update_on_live_connections_and_survive_reconnect_and_holdover() {
    let engine = Engine::new().unwrap();
    let initial = [0x12; 16];
    let leader = engine
        .leader(LeaderConfig {
            bind: "127.0.0.1:0".parse().unwrap(),
            advertise: false,
            session_id: Some(initial),
            ..Default::default()
        })
        .unwrap();
    let lifetime = leader.info().session;
    let mut local = leader.reader().unwrap();
    assert_eq!(local.read().session_id, Some(initial));
    let frozen = local.snapshot();
    let follower = engine
        .follower(FollowerConfig::direct(leader.info().address))
        .unwrap();
    let mut remote = follower.reader().unwrap();
    let before = wait_for(&mut remote, |r| {
        r.status.synchronization == SyncState::Synchronized
    });
    assert_eq!(before.session_id, Some(initial));
    let next = leader.rotate_session_id().unwrap();
    assert_ne!(next, initial);
    assert_eq!(next[6] >> 4, 4); // UUID v4
    assert_eq!(next[8] >> 6, 2); // RFC variant
    assert_eq!(local.read().session_id, Some(next)); // published before acknowledgement
    let after = wait_for(&mut remote, |r| r.session_id == Some(next));
    assert_eq!(after.status.connection, ConnectionState::Connected);
    assert_eq!(after.status.synchronization, SyncState::Synchronized);
    assert!(after.status.accepted_observations >= before.status.accepted_observations);
    assert_eq!(after.discontinuity, before.discontinuity);
    assert_eq!(after.position, before.position); // paused timecode reused in a new part
    assert_eq!(leader.info().session, lifetime);
    assert_eq!(
        frozen.evaluate(engine.clock().now_ns()).session_id,
        Some(initial)
    );

    leader.set_session_id([0; 16]).unwrap(); // caller may explicitly use a nil UUID
    wait_for(&mut remote, |r| r.session_id == Some([0; 16]));
    follower.reconnect().unwrap();
    wait_for(&mut remote, |r| {
        r.status.connection == ConnectionState::Connected
    });
    assert_eq!(remote.read().session_id, Some([0; 16]));
    // A new connection receives the current part, not the startup part.
    let late = engine
        .follower(FollowerConfig::direct(leader.info().address))
        .unwrap();
    let mut late_reader = late.reader().unwrap();
    wait_for(&mut late_reader, |r| r.session_id == Some([0; 16]));
    leader.shutdown().unwrap();
    wait_for(&mut remote, |r| {
        r.status.connection != ConnectionState::Connected
    });
    assert_eq!(remote.read().session_id, Some([0; 16]));
    engine.shutdown().unwrap();
}

#[test]
fn default_ids_are_generated_at_each_start_even_from_cloned_config() {
    let engine = Engine::new().unwrap();
    let config = LeaderConfig {
        advertise: false,
        ..Default::default()
    };
    let first = engine.leader(config.clone()).unwrap();
    let second = engine.leader(config).unwrap();
    let a = first.reader().unwrap().read().session_id.unwrap();
    let b = second.reader().unwrap().read().session_id.unwrap();
    assert_ne!(a, b);
    assert_eq!(a[6] >> 4, 4);
    assert_eq!(b[6] >> 4, 4);
    engine.shutdown().unwrap();
}
