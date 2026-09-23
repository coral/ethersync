#![allow(dead_code)]
use std::{collections::BTreeMap, io::BufRead, time::Duration};
use tidkod::*;
pub fn args() -> BTreeMap<String, String> {
    let mut result = BTreeMap::new();
    let mut args = std::env::args().skip(1).peekable();
    while let Some(key) = args.next() {
        let value = if args.peek().is_some_and(|v| !v.starts_with("--")) {
            args.next().unwrap()
        } else {
            "true".into()
        };
        result.insert(key, value);
    }
    result
}
pub fn format(
    args: &BTreeMap<String, String>,
) -> std::result::Result<FrameFormat, Box<dyn std::error::Error>> {
    let fps = args.get("--fps").map(String::as_str).unwrap_or("30");
    let (n, d) = match fps {
        "23.976" => (24000, 1001),
        "29.97" => (30000, 1001),
        "47.952" => (48000, 1001),
        "59.94" => (60000, 1001),
        n => (n.parse()?, 1),
    };
    Ok(FrameFormat::new(n, d, args.contains_key("--drop-frame"))?)
}
pub fn duration(
    args: &BTreeMap<String, String>,
) -> std::result::Result<Option<Duration>, Box<dyn std::error::Error>> {
    Ok(args
        .get("--seconds")
        .map(|s| s.parse::<u64>().map(Duration::from_secs))
        .transpose()?)
}
pub fn input() -> std::sync::mpsc::Receiver<String> {
    let (tx, rx) = std::sync::mpsc::sync_channel(16);
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            match line {
                Ok(line) => {
                    if tx.send(line).is_err() {
                        return;
                    }
                }
                Err(_) => return,
            }
        }
    });
    rx
}
pub fn print(r: Reading) {
    println!(
        "{} {:+.3}x {:?}/{:?} {:?}/{:?} uncertainty={:.3}ms age={:.2}s RTT={:.3}ms offset={:.3}ms drift={:+.1}ppm loss={}",
        r.label(),
        r.rate.as_f64(),
        r.status.connection,
        r.status.synchronization,
        r.status.source_kind,
        r.status.source_health,
        r.status.uncertainty_ns / 1e6,
        r.status.sample_age_ns as f64 / 1e9,
        r.status.rtt_ns as f64 / 1e6,
        r.status.offset_ns / 1e6,
        r.status.drift_ppm,
        r.status.lost_packets
    );
}
