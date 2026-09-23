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
