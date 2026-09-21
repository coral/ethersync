use ethersync::*;
use std::{
    thread,
    time::{Duration, Instant},
};
#[test]
#[ignore = "requires a multicast-capable network interface; run explicitly"]
fn mdns_advertise_resolve_duplicate_names_and_withdraw() {
    let engine = Engine::new().unwrap();
    let mut d = engine.discovery(DiscoveryConfig::default()).unwrap();
    let a = engine
        .leader(LeaderConfig {
            name: "🎬".repeat(15),
            ..Default::default()
        })
        .unwrap();
    let b = engine
        .leader(LeaderConfig {
            name: "🎬".repeat(15),
            ..Default::default()
        })
        .unwrap();
    let start = Instant::now();
    loop {
        let leaders = d.poll();
        let matches: Vec<_> = leaders
            .iter()
            .filter(|x| x.identity == a.info().identity || x.identity == b.info().identity)
            .collect();
        if matches.len() == 2 {
            for l in matches {
                assert_eq!(l.protocol_version, 1);
                assert!(!l.addresses.is_empty());
                assert_eq!(l.name, "🎬".repeat(15));
                assert!(FollowerConfig::discovered(l, l.addresses[0]).is_ok());
            }
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(12),
            "mDNS resolve timeout: {leaders:?}"
        );
        thread::sleep(Duration::from_millis(100));
    }
    a.shutdown().unwrap();
    let start = Instant::now();
    loop {
        let leaders = d.poll();
        if !leaders.iter().any(|x| x.identity == a.info().identity) {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(8),
            "mDNS withdrawal timeout"
        );
        thread::sleep(Duration::from_millis(100));
    }
    d.shutdown().unwrap();
    engine.shutdown().unwrap();
}
