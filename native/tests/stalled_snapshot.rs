//! A partially sent state frame must not block the follower's clock exchanges.
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};
use tidkod::{Engine, Event, FollowerConfig};
use tidkod_protocol::{decode_probe, encode, timeline::Timeline};
#[test]
fn partial_snapshot_keeps_clock_probes_serviceable() {
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let (stalled_tx, stalled_rx) = mpsc::sync_channel(1);
    let server = std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
            tokio::time::timeout(Duration::from_secs(8),async{
                let mut config=moq_native::ServerConfig::default();config.bind=Some("127.0.0.1:0".into());config.version=vec!["moq-lite-05".parse().unwrap()];config.tls.generate=vec!["localhost".into()];
                let mut server=config.init().unwrap();ready_tx.send(server.local_addr().unwrap()).unwrap();
                let origin=||moq_net::origin::Info::new(moq_net::Origin::random()).produce();let publish=origin();let ingest=origin();
                let mut broadcast=publish.create_broadcast("tidkod/v1",moq_net::broadcast::Route::new().with_announce(true)).unwrap();
                let mut states=broadcast.create_track("state",None).unwrap();let mut replies=broadcast.create_track("clock/reply",None).unwrap();
                states.write_frame(moq_net::Timestamp::now(),encode(&Timeline{revision:1,session:[1;16],..Default::default()}.wire()).unwrap()).unwrap();
                let request=server.accept().await.unwrap();let _session=request.with_publisher(&publish).with_subscriber(ingest.clone()).ok().await.unwrap();
                let incoming=ingest.consume().announced_broadcast("tidkod/v1").await.unwrap();
                let mut probes=incoming.track("clock/request").unwrap().subscribe(None).await.unwrap();
                let epoch=Instant::now();let now=||epoch.elapsed().as_nanos() as u64+1;
                let mut partial_group=None;
                // First serve valid state/probes, then expose an unfinished independent group.
                while epoch.elapsed()<Duration::from_secs(1){
                    let d=probes.recv_datagram().await.unwrap().unwrap();let mut p=decode_probe(&d.payload).unwrap();p.t2=now();p.t3=now();replies.append_datagram(moq_net::Timestamp::now(),encode(&p).unwrap()).unwrap();
                }
                partial_group.get_or_insert_with(||states.append_group().unwrap());
                let mut frame=partial_group.as_mut().unwrap().create_frame(moq_net::frame::Info{size:100,timestamp:moq_net::Timestamp::now()}).unwrap();frame.write([8_u8].as_slice()).unwrap();
                stalled_tx.send(()).unwrap();
                let end=tokio::time::sleep(Duration::from_secs(1));tokio::pin!(end);
                loop{tokio::select!{
                    _=&mut end=>break,
                    d=probes.recv_datagram()=>{let d=d.unwrap().unwrap();let mut p=decode_probe(&d.payload).unwrap();p.t2=now();p.t3=now();replies.append_datagram(moq_net::Timestamp::now(),encode(&p).unwrap()).unwrap();}
                }}
            }).await.unwrap();
        });
    });
    let engine = Engine::new().unwrap();
    let mut config = FollowerConfig::direct(ready_rx.recv().unwrap());
    config.clock_diagnostics = true;
    let follower = engine.follower(config).unwrap();
    stalled_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    while follower.try_event().is_some() {}
    let deadline = Instant::now() + Duration::from_millis(850);
    let mut observations = 0;
    while Instant::now() < deadline {
        while let Some(event) = follower.try_event() {
            if let Event::ClockObservation(_) = event {
                observations += 1;
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        observations >= 2,
        "partial frame blocked probes: {observations}"
    );
    server.join().unwrap();
    engine.shutdown().unwrap();
}
