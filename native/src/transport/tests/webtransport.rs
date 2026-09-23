//! Independent, published MoQ/Tokio client; the server never starts an executor.
use crate::transport::{Endpoint, moq, runtime::Runtime, tls, web};
use std::{
    future::Future,
    task::{Context, Poll, Waker},
    time::{Duration, Instant},
};

#[test]
fn published_client_reads_http3_snapshot_from_polling_server() {
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
    let (stop_tx, stop_rx) = std::sync::mpsc::sync_channel(1);
    let server = std::thread::spawn(move || {
        let (config, fingerprint) = tls::server().unwrap();
        let mut endpoint = Endpoint::new("127.0.0.1:0".parse().unwrap(), Some(config), 16).unwrap();
        ready_tx
            .send((endpoint.local_addr().unwrap(), fingerprint))
            .unwrap();
        let runtime = Runtime::default();
        let web = web::Driver::new(runtime.timers());
        let (source, mut origin_driver) = moq::origin::Producer::new(Default::default());
        let broadcast = source.create_broadcast("ethersync/v1").unwrap();
        let mut track = broadcast.create_track("state", None).unwrap();
        track
            .write_frame(
                moq::Timestamp::from_secs(0).unwrap(),
                b"snapshot".as_slice(),
            )
            .unwrap();
        broadcast.announce(Default::default()).unwrap();
        let mut incoming = None;
        let mut handshake = None;
        let mut session = None;
        let mut cx = Context::from_waker(Waker::noop());
        let mut park = moq::kio::Park::default();
        let deadline = Instant::now() + Duration::from_secs(8);
        while stop_rx.try_recv().is_err() {
            assert!(Instant::now() < deadline, "polling server deadline");
            endpoint.step(Instant::now()).unwrap();
            runtime.timers().advance(Instant::now());
            if incoming.is_none() && handshake.is_none() && session.is_none() {
                incoming = endpoint.accept();
            }
            if let Some(connection) = incoming.as_ref().and_then(|p| p.ready().unwrap()) {
                incoming = None;
                let web = web.clone();
                let source = source.clone();
                let runtime = runtime.clone();
                handshake = Some(Box::pin(async move {
                    let request = web::Request::accept(&web, connection).await.unwrap();
                    let session = request
                        .respond(web::Response::default().with_protocol("moq-lite-05"))
                        .await
                        .unwrap();
                    let (session, driver) = moq::Server::new()
                        .with_publisher(&source)
                        .accept_lite(Instant::now(), session)
                        .await
                        .unwrap();
                    runtime.spawn(driver);
                    session
                }));
            }
            if let Some(pending) = handshake.as_mut()
                && let Poll::Ready(s) = pending.as_mut().poll(&mut cx)
            {
                session = Some(s);
                handshake = None;
            }
            runtime.step(&mut cx, &mut park);
            let _ = origin_driver.poll(Instant::now(), park.hold(&cx));
            web.step(&mut cx, Instant::now());
            std::thread::sleep(Duration::from_micros(100));
        }
        runtime.clear();
        web.clear();
    });
    let (address, fingerprint) = ready_rx.recv().unwrap();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let outcome = rt.block_on(async {
        tokio::time::timeout(Duration::from_secs(5), async {
            let ingest = moq_net::origin::Info::new(moq_net::Origin::random()).produce();
            let mut config = moq_native::ClientConfig::default();
            config.bind = "127.0.0.1:0".parse().unwrap();
            config.version = vec!["moq-lite-05".parse().unwrap()];
            config.tls.fingerprint = vec![fingerprint];
            let client = config.init().unwrap().with_subscriber(ingest.clone());
            let _session = client
                .connect(format!("https://{address}/").parse().unwrap())
                .await
                .unwrap();
            let broadcast = ingest
                .consume()
                .announced_broadcast("ethersync/v1")
                .await
                .unwrap();
            let mut track = broadcast
                .track("state")
                .unwrap()
                .subscribe(None)
                .await
                .unwrap();
            let mut group = track.recv_group().await.unwrap().unwrap();
            let frame = group
                .next_frame()
                .await
                .unwrap()
                .unwrap()
                .read_all()
                .await
                .unwrap();
            assert_eq!(&frame[..], b"snapshot");
        })
        .await
    });
    stop_tx.send(()).unwrap();
    server.join().unwrap();
    outcome.unwrap();
}
