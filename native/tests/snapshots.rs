use std::time::{Duration, Instant};
use tidkod::*;

#[test]
fn frozen_state_survives_refresh_shutdown_and_owner_destruction() {
    let before_engine = Instant::now() - Duration::from_millis(1);
    let engine = Engine::new().unwrap();
    assert_eq!(engine.clock().ns_at(before_engine), None);
    let leader = engine
        .leader(LeaderConfig {
            advertise: false,
            ..Default::default()
        })
        .unwrap();
    let candidates = leader.info().local_endpoints().unwrap();
    assert!(
        candidates
            .iter()
            .all(|a| !a.ip().is_unspecified() && a.port() == leader.info().address.port())
    );
    let address = *candidates.iter().find(|a| a.ip().is_loopback()).unwrap();
    let mut config = FollowerConfig::direct(address);
    config.trust = Trust::Pinned(leader.info().fingerprint.clone());
    let follower = engine.follower(config).unwrap();
    let mut remote = follower.reader().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while remote.read().status.synchronization != SyncState::Synchronized {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(remote.read().status.accepted_observations >= 12);
    let mut reader = leader.reader().unwrap();
    leader
        .seek(Position {
            frames: -7,
            subframe: 0x80000000,
        })
        .unwrap();
    let frozen = reader.snapshot();
    let copied = frozen;
    let now = engine.clock().ns_at(Instant::now()).unwrap();
    assert_eq!(frozen.evaluate(now).position, reader.read_at(now).position);
    assert_eq!(frozen.evaluate(now).status.accepted_observations, 0);
    leader.seek(Position::from_frames(42)).unwrap();
    assert_eq!(reader.snapshot().evaluate(now).position.frames, 42);
    engine.shutdown().unwrap();
    assert_eq!(reader.read().status.connection, ConnectionState::Shutdown);
    drop((reader, remote, follower, leader, engine));
    assert_eq!(
        copied.evaluate(now).position,
        Position {
            frames: -7,
            subframe: 0x80000000
        }
    );
    assert_eq!(
        copied.evaluate(now).status.connection,
        ConnectionState::Connected
    );
}
