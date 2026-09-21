//! Interactive generated leader; all synchronization uses the public API.
mod common;
mod terminal;
use chrono::Timelike;
use libethersync::*;
use std::{
    io::IsTerminal,
    time::{Duration, Instant},
};
fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let args = common::args();
    if args.contains_key("--help") {
        println!(
            "leader [--bind 0.0.0.0:4443] [--fps 29.97] [--drop-frame] [--start FRAMES] [--seconds N] [--no-mdns] [--plain]\nCommands: play, pause, time (or tod), seek FRAMES, shuttle NUM DEN, at DELAY_MS FRAMES NUM DEN, quit"
        );
        return Ok(());
    }
    let engine = Engine::new()?;
    let config = LeaderConfig {
        bind: args
            .get("--bind")
            .map(String::as_str)
            .unwrap_or("0.0.0.0:4443")
            .parse()?,
        format: common::format(&args)?,
        position: Position::from_frames(
            args.get("--start")
                .map(String::as_str)
                .unwrap_or("0")
                .parse()?,
        ),
        advertise: !args.contains_key("--no-mdns"),
        ..Default::default()
    };
    let format = config.format;
    let leader = engine.leader(config)?;
    let mut reader = leader.reader()?;
    let started = Instant::now();
    let limit = common::duration(&args)?;
    let interactive = std::io::stdin().is_terminal()
        && std::io::stdout().is_terminal()
        && std::env::var("TERM").as_deref() != Ok("dumb")
        && !args.contains_key("--plain");
    if interactive {
        let panel = terminal::Panel {
            title: "LEADER",
            address: format!("Listening {}", leader.info().address),
            certificate: format!("SHA-256 {}", leader.info().fingerprint),
            commands: [
                "play | pause | time / tod | seek FRAMES | shuttle NUM DEN",
                "at DELAY_MS FRAMES NUM DEN | quit     Up/Down: history",
            ],
            diagnostics: false,
        };
        let mut terminal = terminal::Terminal::enter()?;
        loop {
            if limit.is_some_and(|l| started.elapsed() >= l) {
                break;
            }
            while let Some(event) = leader.try_event() {
                terminal.message = format!("{event:?}");
            }
            let wait = terminal.draw_live(&panel, &mut reader, engine.clock())?;
            if let Some(line) = terminal.poll(wait)? {
                match command(&engine, &leader, format, &line) {
                    Ok(true) => break,
                    Ok(false) => terminal.message = format!("Applied: {line}"),
                    Err(e) => terminal.message = format!("Error: {e}"),
                }
            }
        }
    } else {
        println!(
            "Listening {} fingerprint {}",
            leader.info().address,
            leader.info().fingerprint
        );
        println!(
            "Commands: play, pause, time (or tod), seek FRAMES, shuttle NUM DEN, at DELAY_MS FRAMES NUM DEN, quit"
        );
        let input = common::input();
        loop {
            if limit.is_some_and(|l| started.elapsed() >= l) {
                break;
            }
            if let Ok(line) = input.try_recv() {
                match command(&engine, &leader, format, &line) {
                    Ok(true) => break,
                    Ok(false) => {}
                    Err(e) => eprintln!("{e}"),
                }
            }
            common::print(reader.read());
            while let Some(event) = leader.try_event() {
                eprintln!("{event:?}");
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    engine.shutdown()?;
    Ok(())
}

fn command(
    engine: &Engine,
    leader: &Leader,
    format: FrameFormat,
    line: &str,
) -> std::result::Result<bool, Box<dyn std::error::Error>> {
    let words: Vec<_> = line.split_whitespace().collect();
    match words.as_slice() {
        ["play"] => leader.play()?,
        ["pause"] => leader.pause()?,
        ["time"] | ["tod"] => {
            // Pair wall time with the engine epoch, then retain that timestamp in the
            // anchor so command queue delay does not make everyone's timecode late.
            let before = engine.clock().now_ns();
            let wall = chrono::Local::now();
            let after = engine.clock().now_ns();
            let sampled_at = before + (after - before) / 2;
            let milliseconds = u64::from(wall.num_seconds_from_midnight()) * 1000
                + u64::from(wall.timestamp_subsec_millis());
            let position =
                Position::ZERO.advance((milliseconds * 1_000_000) as i64, format, Rate::NORMAL);
            leader.set_transport(position, Rate::NORMAL, Some(sampled_at))?;
        }
        ["seek", p] => leader.seek(Position::from_frames(p.parse()?))?,
        ["shuttle", n, d] => leader.speed(Rate::new(n.parse()?, d.parse()?)?)?,
        ["at", delay, p, n, d] => {
            let effective = delay
                .parse::<u64>()?
                .checked_mul(1_000_000)
                .and_then(|delay| engine.clock().now_ns().checked_add(delay))
                .ok_or("Scheduled delay is too large")?;
            leader.set_transport(
                Position::from_frames(p.parse()?),
                Rate::new(n.parse()?, d.parse()?)?,
                Some(effective),
            )?;
        }
        ["quit"] => return Ok(true),
        [] => {}
        _ => {
            return Err(
                "Unknown command. Use play, pause, time/tod, seek, shuttle, at, or quit.".into(),
            );
        }
    }
    Ok(false)
}
