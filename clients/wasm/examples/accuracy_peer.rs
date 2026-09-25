//! Native leader + HTTP/3 byte transport for the Node/WASM accuracy investigation.
//! The independent CAL/SAMPLE pipe never feeds the follower's clock estimator.
use std::io::{BufRead, Write};
use std::time::Duration;
use tidkod::{Engine, LeaderConfig, Position, Rate};
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}
fn output(s: String) {
    let mut out = std::io::stdout().lock();
    writeln!(out, "{s}").unwrap();
    out.flush().unwrap();
}
fn main() {
    if std::env::args().any(|s| s == "--direct") {
        direct();
        return;
    }
    let engine = Engine::new().unwrap();
    let leader = engine
        .leader(LeaderConfig {
            bind: "127.0.0.1:0".parse().unwrap(),
            advertise: false,
            ..Default::default()
        })
        .unwrap();
    leader
        .set_transport(Position::from_frames(1000), Rate::NORMAL, None)
        .unwrap();
    let mut reader = leader.reader().unwrap();
    let (tx, mut commands) = tokio::sync::mpsc::channel(64);
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            if tx.blocking_send(line.unwrap()).is_err() {
                break;
            }
        }
    });
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let origin = || moq_net::origin::Info::new(moq_net::Origin::random()).produce();
        let publish = origin(); let ingest = origin();
        let mut b = publish.create_broadcast("tidkod/v1", moq_net::broadcast::Route::new().with_announce(true)).unwrap();
        let mut requests = b.create_track("clock/request", None).unwrap();
        let mut config = moq_native::ClientConfig::default();
        config.bind = "127.0.0.1:0".parse().unwrap();
        config.version = vec!["moq-lite-05".parse().unwrap()];
        config.tls.fingerprint = vec![leader.info().fingerprint.clone()];
        let client = config.init().unwrap().with_publisher(&publish).with_subscriber(ingest.clone());
        let session = client.connect(format!("https://{}/", leader.info().address).parse().unwrap()).await.unwrap();
        let incoming = ingest.consume().announced_broadcast("tidkod/v1").await.unwrap();
        let mut states = incoming.track("state").unwrap().subscribe(None).await.unwrap();
        let mut replies = incoming.track("clock/reply").unwrap().subscribe(None).await.unwrap();
        output("READY".into());
        let deadline = tokio::time::sleep(Duration::from_secs(90)); tokio::pin!(deadline);
        loop {
            tokio::select! {
                _=&mut deadline => panic!("accuracy fixture timed out"),
                _=session.closed() => panic!("HTTP/3 session ended"),
                line=commands.recv() => {
                    let Some(line)=line else {break};
                    let parts: Vec<_> = line.split_whitespace().collect();
                    match parts.as_slice() {
                        ["CAL", id] => output(format!("CAL {id} {}",engine.clock().now_ns())),
                        ["SAMPLE", id, at] => {
                            let r=reader.read_at(at.parse().unwrap());
                            output(format!("SAMPLE {id} {:.12} {}",r.position.as_frames(),r.discontinuity));
                        },
                        ["RATE", id, numerator, denominator] => {
                            leader.speed(Rate::new(numerator.parse().unwrap(),denominator.parse().unwrap()).unwrap()).unwrap();
                            output(format!("RATE {id}"));
                        },
                        ["PROBE", bytes] => {requests.append_datagram(moq_net::Timestamp::now(),unhex(bytes)).unwrap();},
                        ["QUIT"] => break,
                        _ => panic!("bad fixture command"),
                    }
                },
                group=states.recv_group() => {
                    let mut group=group.unwrap().unwrap();
                    let bytes=group.next_frame().await.unwrap().unwrap().read_all().await.unwrap();
                    output(format!("STATE {}",hex(&bytes)));
                },
                d=replies.recv_datagram() => output(format!("REPLY {}",hex(&d.unwrap().unwrap().payload))),
            }
        }
    });
    engine.shutdown().unwrap();
}

// A real browser connects directly to this leader. The pipe is ONLY an independent
// reference, never a relay for the browser's synchronization messages.
fn direct() {
    use std::time::{Instant, SystemTime, UNIX_EPOCH};
    use tidkod::{FollowerConfig, SourceKind, SourceSample};
    let tracked = std::env::args().any(|s| s == "--tracked");
    let engine = Engine::new().unwrap();
    let leader = engine
        .leader(LeaderConfig {
            bind: "127.0.0.1:0".parse().unwrap(),
            advertise: false,
            source_kind: if tracked {
                SourceKind::Tracked
            } else {
                SourceKind::Generated
            },
            rate: Rate::NORMAL,
            ..Default::default()
        })
        .unwrap();
    let mut reader = leader.reader().unwrap();
    let remote = Engine::new().unwrap();
    let follower = remote
        .follower(FollowerConfig::direct(leader.info().address))
        .unwrap();
    let mut native = follower.reader().unwrap();
    let instant = Instant::now();
    let epoch_delta = i128::from(remote.clock().ns_at(instant).unwrap())
        - i128::from(engine.clock().ns_at(instant).unwrap());
    let (tx, rx) = std::sync::mpsc::sync_channel(64);
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            if tx.send(line.unwrap()).is_err() {
                break;
            }
        }
    });
    output(format!(
        "READY {} {} {}",
        leader.info().address,
        leader.info().fingerprint,
        tidkod::CORE_BUILD_ID
    ));
    let mut next_source = Instant::now();
    let mut first = true;
    let deadline = Instant::now() + Duration::from_secs(7200);
    while Instant::now() < deadline {
        if tracked && Instant::now() >= next_source {
            let before = engine.clock().now_ns();
            let wall = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
                % 86_400_000_000_000;
            let after = engine.clock().now_ns();
            leader
                .track(SourceSample {
                    timestamp_ns: before + (after - before) / 2,
                    position: Position::ZERO.advance(wall as i64, Default::default(), Rate::NORMAL),
                    rate_hint: Some(Rate::NORMAL),
                    discontinuity: first,
                })
                .unwrap();
            first = false;
            next_source = Instant::now() + Duration::from_millis(50);
        }
        let line = match rx.recv_timeout(Duration::from_millis(5)) {
            Ok(line) => line,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(_) => break,
        };
        let parts: Vec<_> = line.split_whitespace().collect();
        match parts.as_slice() {
            ["CAL", id] => output(format!("CAL {id} {}", engine.clock().now_ns())),
            ["READ", id, at] => {
                let at: u64 = at.parse().unwrap();
                let a = reader.read_at(at);
                let b = native.read_at((i128::from(at) + epoch_delta).try_into().unwrap());
                output(format!(
                    "READ {id} {} {} {} {} {} {} {} {}",
                    a.position.frames,
                    a.position.subframe,
                    a.discontinuity,
                    b.position.frames,
                    b.position.subframe,
                    b.status.correction_frames,
                    b.status.offset_ns,
                    b.status.resync_generation
                ));
            }
            ["QUIT"] => break,
            _ => panic!("bad direct fixture command"),
        }
    }
    remote.shutdown().unwrap();
    engine.shutdown().unwrap();
}
