//! Select a discovered leader or specify a direct socket address.
mod common;
mod terminal;
use ethersync::*;
use std::{
    io::IsTerminal,
    time::{Duration, Instant},
};
fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let args = common::args();
    if args.contains_key("--help") {
        println!(
            "follower [--address 192.168.1.2:4443] [--pin SHA256_HEX] [--seconds N] [--plain]\nCommands: reconnect, quit\nWithout --address, browse for three seconds and select a leader."
        );
        return Ok(());
    }
    let engine = Engine::new()?;
    let mut config = if let Some(addr) = args.get("--address") {
        FollowerConfig::direct(addr.parse()?)
    } else {
        let mut discovery = engine.discovery(DiscoveryConfig::default())?;
        std::thread::sleep(Duration::from_secs(3));
        let leaders = discovery.poll();
        for (i, l) in leaders.iter().enumerate() {
            println!("{}: {} ({}) {:?}", i + 1, l.name, l.identity, l.addresses);
        }
        if leaders.is_empty() {
            return Err("No leaders discovered; use --address for a direct connection".into());
        }
        println!("Select leader number:");
        let mut line = String::new();
        std::io::stdin().read_line(&mut line)?;
        let index: usize = line.trim().parse()?;
        let leader = leaders
            .get(index.checked_sub(1).ok_or("Invalid selection")?)
            .ok_or("Invalid selection")?;
        let address = leader
            .addresses
            .iter()
            .find(|a| a.is_ipv4())
            .or(leader.addresses.first())
            .ok_or("No address")?;
        FollowerConfig::discovered(leader, *address)?
    };
    if let Some(pin) = args.get("--pin") {
        config.trust = Trust::Pinned(pin.clone());
    }
    let panel = terminal::Panel {
        title: "FOLLOWER",
        address: format!("Leader {}", config.address),
        certificate: match &config.trust {
            Trust::Pinned(pin) => format!("SHA-256 {pin}"),
            Trust::TrustedLan => "Trust: trusted LAN (no certificate pin)".into(),
        },
        commands: ["reconnect | quit", "Up/Down: history     Ctrl-C: quit"],
        diagnostics: true,
    };
    let follower = engine.follower(config)?;
    let mut reader = follower.reader()?;
    let start = Instant::now();
    let limit = common::duration(&args)?;
    let interactive = std::io::stdin().is_terminal()
        && std::io::stdout().is_terminal()
        && std::env::var("TERM").as_deref() != Ok("dumb")
        && !args.contains_key("--plain");
    if interactive {
        let mut terminal = terminal::Terminal::enter()?;
        while limit.is_none_or(|l| start.elapsed() < l) {
            while let Some(event) = follower.try_event() {
                // Routine clock corrections are reflected in the panel, not the command feedback.
                match event {
                    Event::Error(e) => terminal.message = format!("Error: {e}"),
                    Event::Connection(state) => terminal.message = format!("Connection: {state:?}"),
                    Event::SourceHealth(health) => terminal.message = format!("Source: {health:?}"),
                    Event::Correction(_)
                    | Event::ClockObservation(_)
                    | Event::ProbeTiming { .. } => {}
                }
            }
            let wait = terminal.draw_live(&panel, &mut reader, engine.clock())?;
            if let Some(line) = terminal.poll(wait)? {
                match command(&follower, &line) {
                    Ok(true) => break,
                    Ok(false) => terminal.message = "Reconnecting to the selected leader...".into(),
                    Err(e) => terminal.message = format!("Error: {e}"),
                }
            }
        }
    } else {
        let input = common::input();
        while limit.is_none_or(|l| start.elapsed() < l) {
            if let Ok(line) = input.try_recv() {
                match command(&follower, &line) {
                    Ok(true) => break,
                    Ok(false) => {}
                    Err(e) => eprintln!("{e}"),
                }
            }
            common::print(reader.read());
            while let Some(event) = follower.try_event() {
                if let Event::Error(e) = event {
                    eprintln!("{e}");
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    engine.shutdown()?;
    Ok(())
}

fn command(
    follower: &Follower,
    line: &str,
) -> std::result::Result<bool, Box<dyn std::error::Error>> {
    match line.trim() {
        "reconnect" => follower.reconnect()?,
        "quit" => return Ok(true),
        "" => {}
        _ => return Err("Unknown command. Use reconnect or quit.".into()),
    }
    Ok(false)
}
