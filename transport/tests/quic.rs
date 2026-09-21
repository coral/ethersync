use ethersync_transport::{Endpoint, tls};
use std::{
    task::{Context, Waker},
    time::{Duration, Instant},
};
use web_transport_trait::poll::Session;

#[test]
fn directly_driven_quic_datagrams_and_pinning() {
    let (config, pin) = tls::server().unwrap();
    let mut server = Endpoint::new("127.0.0.1:0".parse().unwrap(), Some(config), 16).unwrap();
    let mut client = Endpoint::new("127.0.0.1:0".parse().unwrap(), None, 16).unwrap();
    let pending = client
        .connect(server.local_addr().unwrap(), Some(&pin))
        .unwrap();
    let mut accepted = None;
    let mut cx = Context::from_waker(Waker::noop());
    let deadline = Instant::now() + Duration::from_secs(3);
    let (mut a, mut b) = loop {
        assert!(Instant::now() < deadline, "handshake deadline");
        client.step(Instant::now()).unwrap();
        server.step(Instant::now()).unwrap();
        if accepted.is_none() {
            accepted = server.accept();
        }
        if let (Some(a), Some(b)) = (
            pending.ready().unwrap(),
            accepted.as_ref().and_then(|b| b.ready().unwrap()),
        ) {
            break (a, b);
        }
        std::thread::sleep(Duration::from_micros(100));
    };
    assert_eq!(a.protocol(), Some("moq-lite-05"));
    loop {
        client.step(Instant::now()).unwrap();
        server.step(Instant::now()).unwrap();
        if a.poll_send_datagram(&mut cx, b"probe").is_ready() {
            break;
        }
    }
    loop {
        assert!(Instant::now() < deadline, "datagram deadline");
        client.step(Instant::now()).unwrap();
        server.step(Instant::now()).unwrap();
        if let std::task::Poll::Ready(r) = b.poll_recv_datagram(&mut cx) {
            assert_eq!(&r.unwrap()[..], b"probe");
            break;
        }
    }
    // Empty polls must not continually reschedule an otherwise idle connection.
    let settle = Instant::now() + Duration::from_millis(100);
    while Instant::now() < settle {
        client.step(Instant::now()).unwrap();
        server.step(Instant::now()).unwrap();
        for _ in 0..32 {
            assert!(a.poll_recv_datagram(&mut cx).is_pending());
            assert!(b.poll_recv_datagram(&mut cx).is_pending());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        !client.needs_pass() && !server.needs_pass(),
        "idle polling must park"
    );
    let bad = client
        .connect(server.local_addr().unwrap(), Some(&"00".repeat(32)))
        .unwrap();
    while bad.ready().is_ok() {
        assert!(Instant::now() < deadline, "pin rejection deadline");
        client.step(Instant::now()).unwrap();
        server.step(Instant::now()).unwrap();
    }
}
