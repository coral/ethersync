//! Independent same-instant measurements, including sustained tracked TOD input.
//! Run optimized: cargo run --release -p tidkod --example accuracy -- --tracked
use chrono::Timelike;
use std::time::{Duration, Instant};
use tidkod::{Engine, FollowerConfig, LeaderConfig, Position, Rate, SourceKind, SourceSample};

fn argument(name: &str, default: u64) -> u64 {
    let args: Vec<_> = std::env::args().collect();
    args.windows(2)
        .find(|p| p[0] == name)
        .map_or(default, |p| p[1].parse().expect("integer argument"))
}
fn statistics(values: &[f64]) -> (f64, f64, f64) {
    let mut signed = values.to_vec();
    signed.sort_by(f64::total_cmp);
    let median = signed[signed.len() / 2];
    signed.iter_mut().for_each(|v| *v = v.abs());
    signed.sort_by(f64::total_cmp);
    (
        median,
        signed[signed.len() * 99 / 100],
        *signed.last().unwrap(),
    )
}
fn main() -> tidkod::Result<()> {
    eprintln!("core_build_id={}", tidkod::CORE_BUILD_ID);
    let tracked = std::env::args().any(|s| s == "--tracked");
    let warmup = Duration::from_secs(argument("--warmup-seconds", 300));
    let duration = Duration::from_secs(argument("--seconds", 600));
    assert!(!duration.is_zero());
    let source_engine = Engine::new()?;
    let leader = source_engine.leader(LeaderConfig {
        bind: "127.0.0.1:0".parse().unwrap(),
        advertise: false,
        rate: Rate::NORMAL,
        source_kind: if tracked {
            SourceKind::Tracked
        } else {
            SourceKind::Generated
        },
        ..Default::default()
    })?;
    let mut source = leader.reader()?;
    // Independent engines, workers and epochs. ns_at converts the SAME Instant
    // exactly; the reference does not use either network clock estimator.
    let engines = [Engine::new()?, Engine::new()?];
    let followers = engines
        .iter()
        .map(|e| e.follower(FollowerConfig::direct(leader.info().address)))
        .collect::<tidkod::Result<Vec<_>>>()?;
    let mut readers = followers
        .iter()
        .map(|f| f.reader())
        .collect::<tidkod::Result<Vec<_>>>()?;
    let mut errors = vec![Vec::new(); readers.len()];
    let mut clock_errors = errors.clone();
    let mut correction_max = 0.0f64;
    let began = Instant::now();
    let mut next_source = began;
    let mut next_report = began;
    let mut source_day = None;
    while began.elapsed() < warmup + duration {
        if tracked && Instant::now() >= next_source {
            let before = source_engine.clock().now_ns();
            let wall = chrono::Local::now();
            let after = source_engine.clock().now_ns();
            let day = wall.date_naive();
            let discontinuity = source_day != Some(day);
            source_day = Some(day);
            let ns = i64::from(wall.num_seconds_from_midnight()) * 1_000_000_000
                + i64::from(wall.nanosecond());
            leader.track(SourceSample {
                timestamp_ns: before + (after - before) / 2,
                position: Position::ZERO.advance(ns, Default::default(), Rate::NORMAL),
                rate_hint: Some(Rate::NORMAL),
                discontinuity,
            })?;
            next_source = Instant::now() + Duration::from_millis(50);
        }
        let instant = Instant::now();
        let source_ns = source_engine.clock().ns_at(instant).unwrap();
        let reference = source.read_at(source_ns);
        for (i, reader) in readers.iter_mut().enumerate() {
            let local = engines[i].clock().ns_at(instant).unwrap();
            let r = reader.read_at(local);
            if began.elapsed() >= warmup {
                let ms = (r.position.fixed() - reference.position.fixed()) as f64
                    / 4294967296.
                    / reference.format.fps()
                    * 1000.;
                errors[i].push(ms);
                clock_errors[i].push((local as f64 + r.status.offset_ns - source_ns as f64) / 1e6);
                correction_max = correction_max
                    .max(r.status.correction_frames.abs() / reference.format.fps() * 1000.);
            }
        }
        if Instant::now() >= next_report {
            eprintln!(
                "accuracy tracked={tracked} elapsed={:.0}s samples={}",
                began.elapsed().as_secs_f64(),
                errors[0].len()
            );
            next_report = Instant::now() + Duration::from_secs(30);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let mut passed = true;
    for (i, values) in errors.iter().enumerate() {
        let (median, p99, max) = statistics(values);
        let (clock_median, clock_p99, clock_max) = statistics(&clock_errors[i]);
        println!(
            "{{\"follower\":{i},\"tracked\":{tracked},\"samples\":{},\"medianMs\":{median},\"p99AbsMs\":{p99},\"maxAbsMs\":{max},\"clockMedianMs\":{clock_median},\"clockP99AbsMs\":{clock_p99},\"clockMaxAbsMs\":{clock_max},\"correctionMaxMs\":{correction_max}}}",
            values.len()
        );
        passed &= p99 <= 1. && max <= 2.;
    }
    for e in &engines {
        e.shutdown()?;
    }
    source_engine.shutdown()?;
    assert!(passed, "same-computer timeline exceeded p99=1ms or max=2ms");
    Ok(())
}
