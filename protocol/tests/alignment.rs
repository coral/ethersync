use tidkod_protocol::{
    CorrectionPolicy, Position, Rate,
    clock::Exchange,
    timeline::{Anchor, FollowerCore, Timeline},
};

fn exchange(at: u64) -> Exchange {
    Exchange {
        t1: at,
        t2: at + 100_000,
        t3: at + 110_000,
        t4: at + 210_000,
    }
}
fn acquired(rate: Rate) -> FollowerCore {
    let mut core = FollowerCore::new(Timeline::default(), CorrectionPolicy::default());
    core.connected();
    core.state(
        Timeline {
            session: [1; 16],
            revision: 1,
            anchor: Anchor {
                time_ns: 0,
                position: Position::ZERO,
                rate,
            },
            ..Default::default()
        },
        1,
    );
    for i in 0..3580 {
        core.measurement(exchange(1_000_000_000 + i * 50_000_000));
    }
    core
}

#[test]
fn steady_state_phase_error_settles_within_one_second_despite_heartbeats() {
    for rate in [Rate::NORMAL, Rate::new(-1, 1).unwrap()] {
        for ms in [-30., -20., -5., 5., 20., 30.] {
            let mut c = acquired(rate);
            let now = 180_000_000_000;
            // A tracked leader refines its trajectory after MINUTES of playback.
            let mut state = c.view.timeline;
            state.revision += 1;
            state.anchor.position = Position::from_fixed((ms / 1000. * 30. * 4294967296.) as i128);
            let before = c.view.evaluate(now).position;
            c.state(state, now);
            assert_eq!(
                c.view.evaluate(now).position,
                before,
                "ordinary correction must start continuously"
            );
            let mut previous = before;
            for i in 1..=200 {
                let t = now + i * 5_000_000;
                if i % 10 == 0 {
                    c.measurement(exchange(t - 210_000));
                }
                if i % 20 == 0 {
                    state.revision += 1;
                    c.state(state, t);
                }
                let reading = c.view.evaluate(t);
                assert_eq!(reading.position > previous, rate.numerator() > 0);
                previous = reading.position;
            }
            let remaining_ms = c
                .view
                .evaluate(now + 1_000_000_000)
                .status
                .correction_frames
                .abs()
                / 30.
                * 1000.;
            assert!(
                remaining_ms < 1.,
                "{ms}ms phase step retained {remaining_ms}ms after one second at {rate:?}"
            );
        }
    }
}

#[test]
fn readiness_accounts_for_correction_health_evidence_and_holdover() {
    use tidkod_protocol::{SourceHealth, clock::OffsetEvidence};
    let mut c = acquired(Rate::NORMAL);
    let now = 179_950_210_000;
    assert!(c.view.evaluate(now).status.aligned);
    assert!(!c.needs_fast_probes(now));
    let mut state = c.view.timeline;
    state.revision += 1;
    state.anchor.position = Position::from_fixed((0.6 * 4294967296.) as i128);
    c.state(state, now);
    let reading = c.view.evaluate(now);
    assert!(!reading.status.aligned);
    assert!(reading.status.alignment_error_ns >= 20_000_000.);
    assert!(c.needs_fast_probes(now));
    assert!(c.view.evaluate(now + 250_000_000).status.aligned);
    c.view.timeline.source_health = SourceHealth::Degraded;
    assert!(!c.view.evaluate(now + 250_000_000).status.aligned);
    c.view.timeline.source_health = SourceHealth::Healthy;
    c.view.mapping.evidence = Some(OffsetEvidence {
        reference_ns: now,
        lower_ns: 1.,
        upper_ns: 0.,
        samples: 12,
    });
    assert!(!c.view.evaluate(now + 250_000_000).status.aligned);
    c.view.mapping.evidence = None;
    c.disconnected();
    assert!(!c.view.evaluate(now + 250_000_000).status.aligned);
}

#[test]
fn paused_correction_stays_pending_and_hard_resync_generation_is_persistent() {
    let mut c = acquired(Rate::PAUSED);
    let now = 180_000_000_000;
    let generation = c.view.resync_generation;
    let mut state = c.view.timeline;
    state.revision += 1;
    state.anchor.position = Position::from_fixed(1 << 30);
    c.state(state, now);
    assert!(!c.view.evaluate(now).status.aligned);
    assert_eq!(
        c.view.evaluate(now).position,
        c.view.evaluate(now + 60_000_000_000).position
    );
    assert_eq!(c.view.resync_generation, generation);
    state.revision += 1;
    state.anchor.position = Position::from_frames(30);
    c.state(state, now);
    for i in 0..3 {
        c.measurement(exchange(now + i * 50_000_000));
    }
    assert_eq!(c.view.resync_generation, generation + 1);
    assert_eq!(
        c.view.evaluate(now + 200_000_000).position,
        Position::from_frames(30)
    );
    let frozen = tidkod_protocol::TimecodeSnapshot::from(c.view);
    c.measurement(exchange(now + 250_000_000));
    assert_eq!(
        frozen.evaluate(now + 500_000_000).status.resync_generation,
        generation + 1
    );
    assert_eq!(c.view.resync_generation, generation + 1);
}

#[test]
fn quarantine_uses_fast_probes_before_reacquisition() {
    let mut c = acquired(Rate::NORMAL);
    let now = 180_000_000_000;
    assert!(!c.needs_fast_probes(now));
    let mut e = exchange(now);
    e.t2 += 100_000_000;
    e.t3 += 100_000_000;
    c.measurement(e);
    assert!(c.estimator.recovering());
    assert!(!c.view.evaluate(e.t4).status.aligned);
    assert!(c.needs_fast_probes(e.t4));
    c.measurement(exchange(now + 50_000_000));
    assert!(!c.estimator.recovering());
    assert!(!c.needs_fast_probes(now + 50_210_000));
}

#[test]
fn processing_a_delayed_measurement_preserves_output_at_processing_time() {
    let mut c = acquired(Rate::NORMAL);
    let mut e = exchange(180_000_000_000);
    e.t2 += 2_000_000;
    e.t3 += 2_000_000;
    let processed = e.t4 + 50_000_000;
    let before = c.view.evaluate(processed).position;
    c.measurement_timed(e, None, processed);
    assert_eq!(c.estimator.trace().last().unwrap().exchange.t4, e.t4);
    assert_eq!(
        c.view.evaluate(processed).position,
        before,
        "a queued observation must not retroactively apply 50ms of a new slew"
    );
}
