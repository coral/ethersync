use tidkod_protocol::timeline::Timeline;
use tidkod_protocol::{wire::*, *};
fn unhex(s: &str) -> Vec<u8> {
    let s = s.trim();
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}
#[test]
fn golden_fixtures() {
    let bytes = unhex(include_str!("../fixtures/probe.hex"));
    let p = decode_probe(&bytes).unwrap();
    assert_eq!((p.sequence, p.t1, p.t2, p.t3), (1, 1000, 1500, 1510));
    assert_eq!(encode(&p).unwrap(), bytes);
    assert!(bytes.len() < 64);
    let bytes = unhex(include_str!("../fixtures/paused.hex"));
    let s = decode_snapshot(&bytes).unwrap();
    assert_eq!(s.session, [1; 16]);
    assert_eq!(
        s.anchor.as_ref().unwrap().position.as_ref().unwrap().frames,
        0
    );
    assert_eq!(encode(&s).unwrap(), bytes);
    assert!(bytes.len() < 128);
}
#[test]
fn rejects_invalid_and_oversize() {
    let mut s = decode_snapshot(&unhex(include_str!("../fixtures/paused.hex"))).unwrap();
    s.version = 2;
    assert!(matches!(
        decode_snapshot(&encode(&s).unwrap()),
        Err(Error::Version(2))
    ));
    s.version = 1;
    s.source_kind = 99;
    assert!(validate_snapshot(&s).is_err());
    s.source_kind = 1;
    s.anchor
        .as_mut()
        .unwrap()
        .rate
        .as_mut()
        .unwrap()
        .denominator = 0;
    assert!(validate_snapshot(&s).is_err());
    assert!(matches!(decode_snapshot(&[0; 513]), Err(Error::Size)));
    assert!(matches!(decode_probe(&[0; 513]), Err(Error::Size)));
    assert!(decode_snapshot(&[0x80]).is_err());
    assert!(
        encode(&Snapshot {
            session: vec![0; 513],
            ..Default::default()
        })
        .is_err()
    );
}
#[test]
fn random_malformed_messages_never_panic() {
    let mut seed = 0u64;
    for len in 0..=512 {
        let bytes: Vec<u8> = (0..len)
            .map(|_| {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                (seed >> 32) as u8
            })
            .collect();
        let _ = decode_snapshot(&bytes);
        let _ = decode_probe(&bytes);
    }
}
#[test]
fn schedule_order_and_size() {
    let mut s = decode_snapshot(&unhex(include_str!("../fixtures/paused.hex"))).unwrap();
    for i in 1..=4 {
        let mut a = s.anchor.unwrap();
        a.time_ns = i * 1_000_000_000;
        s.scheduled.push(ScheduledChange {
            discontinuity: i,
            anchor: Some(a),
        });
    }
    validate_snapshot(&s).unwrap();
    assert!(encode(&s).unwrap().len() <= 512);
    s.scheduled[1].discontinuity = 1;
    assert!(validate_snapshot(&s).is_err());
}

#[test]
fn probe_direction_timestamps_are_unambiguous() {
    let p = wire::Probe {
        version: 1,
        sequence: 1,
        t1: 1,
        t2: 0,
        t3: 2,
    };
    assert!(decode_probe(&encode(&p).unwrap()).is_err());
    let p = wire::Probe { t2: 2, t3: 0, ..p };
    assert!(decode_probe(&encode(&p).unwrap()).is_err());
}

#[test]
fn recording_session_id_is_optional_and_exactly_sixteen_bytes() {
    let legacy = decode_snapshot(&unhex(include_str!("../fixtures/paused.hex"))).unwrap();
    assert_eq!(Timeline::from_wire(&legacy).unwrap().session_id, None);
    for len in [1, 15, 17, 32] {
        let mut s = legacy.clone();
        s.session_id = vec![7; len];
        assert!(validate_snapshot(&s).is_err());
    }
    // All UUID bit patterns, including nil, are opaque caller-owned identifiers.
    for id in [[0; 16], [0xff; 16], [7; 16]] {
        let mut s = legacy.clone();
        s.session_id = id.to_vec();
        let t = Timeline::from_wire(&decode_snapshot(&encode(&s).unwrap()).unwrap()).unwrap();
        assert_eq!(t.session_id, Some(id));
        assert_eq!(encode(&t.wire()).unwrap(), encode(&s).unwrap());
    }
}

#[test]
fn recording_parts_preserve_slew_schedules_and_connection_identity_rules() {
    use tidkod_protocol::timeline::{FollowerCore, Scheduled};
    let mut core = FollowerCore::new(Timeline::default(), Default::default());
    core.connected();
    let mut timeline = Timeline {
        session: [1; 16],
        session_id: Some([2; 16]),
        revision: 1,
        ..Default::default()
    };
    timeline.scheduled[0] = Scheduled {
        discontinuity: 1,
        anchor: tidkod_protocol::timeline::Anchor {
            time_ns: 10_000_000_000,
            position: tidkod_protocol::Position::from_frames(100),
            rate: tidkod_protocol::Rate::NORMAL,
        },
    };
    timeline.scheduled_len = 1;
    core.state(timeline, 1);
    core.view.correction_frames = 0.25;
    core.view.mapping.accepted_observations = 25;
    core.view.mapping.last_sample_ns = 1;
    timeline.revision = 2;
    timeline.session_id = Some([3; 16]);
    assert!(
        matches!(core.state(timeline, 2), Some(tidkod_protocol::timeline::Correction::Slew { frames }) if frames == -0.25)
    );
    assert_eq!(core.view.correction_frames, 0.25);
    assert_eq!(core.view.mapping.accepted_observations, 25);
    assert_eq!(core.view.timeline.scheduled_len, 1);
    assert_eq!(core.view.timeline.at(10_000_000_000).0.frames, 100);
    assert_eq!(core.view.evaluate(2).session_id, Some([3; 16]));
    timeline.revision = 1;
    timeline.session_id = Some([2; 16]);
    core.state(timeline, 3);
    assert_eq!(core.view.evaluate(3).session_id, Some([3; 16]));
    timeline.revision = 3;
    timeline.session = [9; 16];
    core.state(timeline, 4);
    assert_eq!(core.view.timeline.session, [1; 16]);
    assert_eq!(core.view.evaluate(4).session_id, Some([3; 16]));
}
