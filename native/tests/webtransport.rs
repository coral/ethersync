//! Exercises the leader's browser-facing HTTP/3 endpoint, without a browser.
use std::time::{Duration, Instant};
use tidkod::{Engine, LeaderConfig};
use tidkod_protocol::{decode_probe, decode_snapshot, probes::Probes};

#[test]
fn http3_state_and_private_datagrams_with_pinning() {
    let engine = Engine::new().unwrap();
    let leader = engine
        .leader(LeaderConfig {
            bind: "127.0.0.1:0".parse().unwrap(),
            advertise: false,
            ..Default::default()
        })
        .unwrap();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        tokio::time::timeout(Duration::from_secs(8), async {
            let version: moq_net::Version = "moq-lite-05".parse().unwrap();
            let origin = || moq_net::origin::Info::new(moq_net::Origin::random()).produce();
            let publish = origin();
            let ingest = origin();
            let mut b = publish
                .create_broadcast(
                    "tidkod/v1",
                    moq_net::broadcast::Route::new().with_announce(true),
                )
                .unwrap();
            let mut requests = b.create_track("clock/request", None).unwrap();
            let mut config = moq_native::ClientConfig::default();
            config.bind = "127.0.0.1:0".parse().unwrap();
            config.version = vec![version];
            config.tls.fingerprint = vec![leader.info().fingerprint.clone()];
            let url = format!("https://{}/", leader.info().address)
                .parse()
                .unwrap();
            let client = config
                .init()
                .unwrap()
                .with_publisher(&publish)
                .with_subscriber(ingest.clone());
            let session = client.connect(url).await.unwrap();
            assert_eq!(session.version(), version);
            let incoming = ingest
                .consume()
                .announced_broadcast("tidkod/v1")
                .await
                .unwrap();
            let mut states = incoming
                .track("state")
                .unwrap()
                .subscribe(None)
                .await
                .unwrap();
            let mut replies = incoming
                .track("clock/reply")
                .unwrap()
                .subscribe(None)
                .await
                .unwrap();
            let mut group = states.recv_group().await.unwrap().unwrap();
            let frame = group
                .next_frame()
                .await
                .unwrap()
                .unwrap()
                .read_all()
                .await
                .unwrap();
            assert_eq!(decode_snapshot(&frame).unwrap().version, 1);
            let epoch = Instant::now();
            let now = || epoch.elapsed().as_nanos() as u64;
            let mut probes = Probes::default();
            // Initial datagrams may precede the leader's subscription; they are unreliable.
            let exchange = loop {
                requests
                    .append_datagram(moq_net::Timestamp::now(), probes.request(now()).unwrap())
                    .unwrap();
                if let Ok(Ok(Some(d))) =
                    tokio::time::timeout(Duration::from_millis(100), replies.recv_datagram()).await
                {
                    assert!(decode_probe(&d.payload).unwrap().t2 > 0);
                    break probes.reply(&d.payload, now()).unwrap().unwrap();
                }
            };
            assert!(exchange.t4 >= exchange.t1);
            assert!(exchange.t3 >= exchange.t2);
            drop(session);
            let mut bad = moq_native::ClientConfig::default();
            bad.bind = "127.0.0.1:0".parse().unwrap();
            bad.version = vec![version];
            bad.tls.fingerprint = vec!["00".repeat(32)];
            assert!(
                bad.init()
                    .unwrap()
                    .connect(
                        format!("https://{}/", leader.info().address)
                            .parse()
                            .unwrap()
                    )
                    .await
                    .is_err()
            );
        })
        .await
        .unwrap();
    });
    engine.shutdown().unwrap();
}
