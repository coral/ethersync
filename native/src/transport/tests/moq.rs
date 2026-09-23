use crate::transport::{Endpoint, moq, runtime::Runtime, tls};
use std::{
    future::Future,
    pin::pin,
    task::{Context, Poll, Waker},
    time::{Duration, Instant},
};

#[test]
fn moq_lite_handshake_and_track_without_executor() {
    let (config, fingerprint) = tls::server().unwrap();
    let mut server = Endpoint::new("127.0.0.1:0".parse().unwrap(), Some(config), 16).unwrap();
    let mut client = Endpoint::new("127.0.0.1:0".parse().unwrap(), None, 16).unwrap();
    let pending = client
        .connect(server.local_addr().unwrap(), Some(&fingerprint))
        .unwrap();
    let mut incoming = None;
    let deadline = Instant::now() + Duration::from_secs(3);
    let (a, b) = loop {
        assert!(Instant::now() < deadline);
        client.step(Instant::now()).unwrap();
        server.step(Instant::now()).unwrap();
        if incoming.is_none() {
            incoming = server.accept();
        }
        if let (Some(a), Some(b)) = (
            pending.ready().unwrap(),
            incoming.as_ref().and_then(|p| p.ready().unwrap()),
        ) {
            break (a, b);
        }
    };
    let runtime = Runtime::default();
    let (source, mut source_driver) =
        moq::origin::Producer::new(moq::origin::Config::new(moq::Hop::new(1).unwrap()));
    let (sink, mut sink_driver) =
        moq::origin::Producer::new(moq::origin::Config::new(moq::Hop::new(2).unwrap()));
    let broadcast = source.create_broadcast("tidkod/v1").unwrap();
    let mut track = broadcast.create_track("state", None).unwrap();
    track
        .write_frame(
            moq::Timestamp::from_secs(0).unwrap(),
            b"snapshot".as_slice(),
        )
        .unwrap();
    broadcast.announce(moq::origin::Route::default()).unwrap();
    let client_builder = moq::Client::new().with_subscriber(sink.clone());
    let server_builder = moq::Server::new().with_publisher(&source);
    let mut client_handshake =
        pin!(client_builder.connect_lite(Instant::now(), crate::transport::web::Session::raw(a)));
    let mut server_handshake =
        pin!(server_builder.accept_lite(Instant::now(), crate::transport::web::Session::raw(b)));
    let mut client_session = None;
    let mut server_session = None;
    let receive = async {
        let broadcast = sink.consume().routed_broadcast("tidkod/v1").await.unwrap();
        let mut track = broadcast
            .track("state")
            .unwrap()
            .subscribe(None)
            .await
            .unwrap();
        let mut group = track.recv_group().await.unwrap().unwrap();
        let mut frame = group.next_frame().await.unwrap().unwrap();
        frame.read_all().await.unwrap()
    };
    let mut receive = pin!(receive);
    let mut cx = Context::from_waker(Waker::noop());
    let mut park = moq::kio::Park::default();
    loop {
        assert!(Instant::now() < deadline, "MoQ progress deadline");
        client.step(Instant::now()).unwrap();
        server.step(Instant::now()).unwrap();
        runtime.timers().advance(Instant::now());
        if client_session.is_none()
            && let Poll::Ready(r) = client_handshake.as_mut().poll(&mut cx)
        {
            let (session, driver) = r.unwrap();
            runtime.spawn(driver);
            client_session = Some(session);
        }
        if server_session.is_none()
            && let Poll::Ready(r) = server_handshake.as_mut().poll(&mut cx)
        {
            let (session, driver) = r.unwrap();
            runtime.spawn(driver);
            server_session = Some(session);
        }
        runtime.step(&mut cx, &mut park);
        let waiter = park.hold(&cx);
        let _ = source_driver.poll(Instant::now(), waiter);
        let _ = sink_driver.poll(Instant::now(), waiter);
        if let Poll::Ready(bytes) = receive.as_mut().poll(&mut cx) {
            assert_eq!(&bytes[..], b"snapshot");
            break;
        }
    }
    runtime.clear();
}
