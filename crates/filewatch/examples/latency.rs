//! Benchmark fixture: measure filewatch event delivery latency and loss.
//!
//! Spawns `filewatch DIR --json -d 0ms`, creates N files at RATE files/s, and matches
//! every reported path to its creation time.
//! Usage: cargo run --release --example latency -- <filewatch-binary> <dir> <count> <rate>
#![allow(clippy::unwrap_used)] // Fixture binary: fail fast.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let bin = PathBuf::from(&args[1]);
    let dir = PathBuf::from(&args[2]);
    let count: usize = args[3].parse().unwrap();
    let rate: f64 = args[4].parse().unwrap();

    let mut child = Command::new(bin)
        .arg(&dir)
        .args(["--json", "-d", "0ms"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel::<(String, Instant)>();

    // Reader: timestamp every line the moment it arrives
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let line = line.unwrap();
            let v: serde_json::Value = serde_json::from_str(&line).unwrap();

            if v["event"] == "created" {
                tx.send((v["path"].as_str().unwrap().to_string(), Instant::now()))
                    .unwrap();
            }
        }
    });

    thread::sleep(Duration::from_millis(500));

    let interval = Duration::from_secs_f64(1.0 / rate);
    let start = Instant::now();
    let mut created: HashMap<String, Instant> = HashMap::with_capacity(count);

    for i in 0..count {
        // Pace writes to the requested rate
        let due = start + interval.mul_f64(i as f64);

        if let Some(wait) = due.checked_duration_since(Instant::now()) {
            thread::sleep(wait);
        }

        let name = format!("f{i:06}");
        std::fs::write(dir.join(&name), b"x").unwrap();
        created.insert(name, Instant::now());
    }

    let write_secs = start.elapsed().as_secs_f64();
    let mut latencies: Vec<f64> = Vec::with_capacity(count);
    let deadline = Instant::now() + Duration::from_secs(5);

    while latencies.len() < count {
        let Some(left) = deadline.checked_duration_since(Instant::now()) else {
            break;
        };

        let Ok((path, at)) = rx.recv_timeout(left) else {
            break;
        };

        if let Some(t0) = created.remove(&path) {
            latencies.push(at.saturating_duration_since(t0).as_secs_f64() * 1000.0);
        }
    }

    child.kill().unwrap();
    child.wait().unwrap();

    latencies.sort_by(f64::total_cmp);
    let pct = |q: f64| {
        latencies[((q * latencies.len() as f64).ceil() as usize).clamp(1, latencies.len()) - 1]
    };

    println!(
        "{count} files at {:.0}/s: received {} ({} missing); latency p50 {:.2} ms, p95 {:.2} ms, p99 {:.2} ms, max {:.2} ms",
        count as f64 / write_secs,
        latencies.len(),
        count - latencies.len(),
        pct(0.50),
        pct(0.95),
        pct(0.99),
        latencies.last().copied().unwrap_or_default()
    );
}
