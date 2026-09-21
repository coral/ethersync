//! Simulated external source: jitter, input loss, pause, and reverse playback.
mod common;
use libethersync::*;
use std::time::{Duration, Instant};
fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let args = common::args();
    if args.contains_key("--help") {
        println!(
            "tracked [--bind 0.0.0.0:4443] [--seconds 10] [--no-mdns]\nInput loss at 3–5s, pause at 6s, reverse at 7s."
        );
        return Ok(());
    }
    let engine = Engine::new()?;
    let clock = engine.clock();
    let config = LeaderConfig {
        bind: args
            .get("--bind")
            .map(String::as_str)
            .unwrap_or("0.0.0.0:4443")
            .parse()?,
        source_kind: SourceKind::Tracked,
        advertise: !args.contains_key("--no-mdns"),
        ..Default::default()
    };
    let leader = engine.leader(config)?;
    let mut reader = leader.reader()?;
    println!(
        "Listening {} fingerprint {}",
        leader.info().address,
        leader.info().fingerprint
    );
    let start = Instant::now();
    let limit = common::duration(&args)?.unwrap_or(Duration::from_secs(10));
    let mut tick = 0u64;
    let mut previous_rate = Rate::NORMAL;
    while start.elapsed() < limit {
        let seconds = start.elapsed().as_secs_f64();
        let rate = if seconds < 6. {
            Rate::NORMAL
        } else if seconds < 7. {
            Rate::PAUSED
        } else {
            Rate::new(-1, 1)?
        };
        let frames = if seconds < 6. {
            seconds * 30.
        } else if seconds < 7. {
            180.
        } else {
            180. - (seconds - 7.) * 30.
        };
        if !(3.0..5.0).contains(&seconds) {
            let jitter = if tick.is_multiple_of(2) {
                0.002
            } else {
                -0.002
            };
            leader.track(SourceSample {
                timestamp_ns: clock.now_ns(),
                position: Position::from_fixed(((frames + jitter) * 4294967296.) as i128),
                rate_hint: Some(rate),
                discontinuity: rate != previous_rate,
            })?;
        }
        previous_rate = rate;
        if tick.is_multiple_of(5) {
            common::print(reader.read());
        }
        tick += 1;
        std::thread::sleep(Duration::from_millis(20));
    }
    engine.shutdown()?;
    Ok(())
}
