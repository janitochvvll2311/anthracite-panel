//! Deterministic host overview for the demo (`anthracite:home/host@1.0.0`).
//!
//! The demo instance must not show the real machine's hostname, addresses or
//! storage. This fixture answers the same contract as the real Home service
//! with synthetic values that move over time, so the panels look alive without
//! ever reading `/proc`, `df` or `ip`.
//!
//! The simulation is a function of the seconds since the service started, not
//! of the host clock: metrics rise and fall on a fixed curve, and the counters
//! only ever grow.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use anth_contract::Error;
use anth_service::ChunkStream;
use futures_util::stream::unfold;
use serde_json::{json, Value};

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
const MIB: f64 = 1024.0 * 1024.0;
const KIB: f64 = 1024.0;

pub fn handle(function: &str, _input: &Value) -> Result<Value, Error> {
    match function {
        "info" => Ok(info()),
        other => Err(Error::not_found(format!(
            "`anthracite:home/host#{other}` is not implemented"
        ))),
    }
}

/// One snapshot every `interval-ms` (clamped to 1–60 s), encoded as the JSON
/// chunks the engine frames as SSE.
pub fn watch(input: &Value) -> Result<ChunkStream, Error> {
    let milliseconds = match input.get("interval-ms") {
        Some(Value::Number(value)) => value.as_u64(),
        Some(Value::String(value)) => value.parse::<u64>().ok(),
        _ => None,
    }
    .unwrap_or(2_000)
    .clamp(1_000, 60_000);
    let interval = Duration::from_millis(milliseconds);
    Ok(Box::pin(unfold(
        (true, interval),
        |(first, interval)| async move {
            if !first {
                tokio::time::sleep(interval).await;
            }
            let result = serde_json::to_vec(&info())
                .map_err(|error| Error::internal(format!("encoding host info: {error}")));
            Some((result, (false, interval)))
        },
    )))
}

/// Seconds since the fixture started. Every metric is a function of this.
fn elapsed() -> f64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f64()
}

fn info() -> Value {
    info_at(elapsed())
}

fn info_at(t: f64) -> Value {
    let memory = memory_info(t);
    json!({
        "hostname": "anthracite-demo",
        "os": "Anthracite Demo 1.0",
        "kernel": "6.6.0-demo",
        "arch": "x86_64",
        // 3d 4h 2m at start, then one second per second.
        "uptime-seconds": 273_720u64 + t as u64,
        "cpu": cpu_info(t),
        "memory": memory,
        "disks": disks(t),
        "networks": networks(t),
    })
}

fn cpu_info(t: f64) -> Value {
    // A pair of slow sines keeps usage between roughly 12% and 52%.
    let usage = (32.0 + 14.0 * (t / 7.0).sin() + 4.0 * (t / 2.3).sin()).clamp(2.0, 95.0);
    json!({
        "model": "Anthracite Demo CPU @ 2.40GHz",
        "cores": 8,
        "usage-percent": round(usage, 1),
        "load-1": round(0.8 + 0.35 * (t / 11.0).sin() + 0.08 * (t / 3.0).cos(), 2),
        "load-5": round(0.72 + 0.22 * (t / 17.0 + 0.6).sin(), 2),
        "load-15": round(0.64 + 0.12 * (t / 31.0 + 1.1).sin(), 2),
    })
}

fn memory_info(t: f64) -> Value {
    let total = (16.0 * GIB) as u64;
    let used = 6.4 * GIB + 600.0 * MIB * (t / 13.0).sin() + 120.0 * MIB * (t / 3.1).sin();
    let used = used.clamp(1.5 * GIB, total as f64 - 512.0 * MIB) as u64;
    let swap_total = (4.0 * GIB) as u64;
    let swap_used = (256.0 * MIB + 64.0 * MIB * (t / 17.0).sin()).max(0.0) as u64;
    json!({
        "total": total,
        "used": used,
        "available": total - used,
        "swap-total": swap_total,
        "swap-used": swap_used,
    })
}

fn disks(t: f64) -> Vec<Value> {
    [
        (
            "/dev/vda1",
            "/",
            "ext4",
            256.0 * GIB,
            96.0 * GIB,
            32.0 * MIB,
        ),
        (
            "/dev/vdb1",
            "/data",
            "ext4",
            512.0 * GIB,
            220.0 * GIB,
            20.0 * MIB,
        ),
    ]
    .into_iter()
    .map(|(name, mount, file_system, total, base, growth)| {
        // The filesystems fill slowly but never cross 90%.
        let total = total as u64;
        let used = ((base + growth * t).min(total as f64 * 0.9)) as u64;
        json!({
            "name": name,
            "mount": mount,
            "file-system": file_system,
            "total": total,
            "used": used,
            "available": total - used,
        })
    })
    .collect()
}

fn networks(t: f64) -> Vec<Value> {
    // Counters only grow: a base plus a fixed rate since start.
    let eth_rx = 1_200.0 * MIB + 240.0 * KIB * t;
    let eth_tx = 300.0 * MIB + 60.0 * KIB * t;
    let lo = 1.0 * MIB + 2.0 * KIB * t;
    vec![
        json!({
            "name": "eth0",
            "mac": "02:00:00:00:00:10",
            "addresses": ["10.0.0.10/24"],
            "state": "up",
            "received": eth_rx as u64,
            "transmitted": eth_tx as u64,
            "packets-received": (460_000.0 + 45.0 * t) as u64,
            "packets-transmitted": (112_000.0 + 12.0 * t) as u64,
            "errors": 0,
        }),
        json!({
            "name": "lo",
            "mac": Value::Null,
            "addresses": ["127.0.0.1/8"],
            "state": "unknown",
            "received": lo as u64,
            "transmitted": lo as u64,
            "packets-received": (1_200.0 + 3.0 * t) as u64,
            "packets-transmitted": (1_200.0 + 3.0 * t) as u64,
            "errors": 0,
        }),
    ]
}

/// Rounds to `places` decimals without pulling a float formatting dependency
/// into the JSON value.
fn round(value: f64, places: i32) -> f64 {
    let factor = 10f64.powi(places);
    (value * factor).round() / factor
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fixture_moves_without_reading_the_host() {
        let first = info_at(0.0);
        let second = info_at(10.0);
        assert!(
            second["uptime-seconds"].as_u64() > first["uptime-seconds"].as_u64(),
            "uptime must advance"
        );
        assert!(
            second["networks"][0]["received"].as_u64() > first["networks"][0]["received"].as_u64(),
            "network counters must only grow"
        );
        assert!(
            second["disks"][0]["used"].as_u64() >= first["disks"][0]["used"].as_u64(),
            "disk usage must not decrease"
        );
        for info in [&first, &second] {
            let memory = &info["memory"];
            assert_eq!(
                memory["used"].as_u64().unwrap() + memory["available"].as_u64().unwrap(),
                memory["total"].as_u64().unwrap()
            );
            for disk in info["disks"].as_array().unwrap() {
                assert_eq!(
                    disk["used"].as_u64().unwrap() + disk["available"].as_u64().unwrap(),
                    disk["total"].as_u64().unwrap()
                );
            }
            let usage = info["cpu"]["usage-percent"].as_f64().unwrap();
            assert!((0.0..=100.0).contains(&usage));
        }
        assert_eq!(first["hostname"], "anthracite-demo");
    }

    #[test]
    fn the_contract_answers_info_and_refuses_unknown_functions() {
        let info = handle("info", &json!({})).expect("info");
        assert_eq!(info["cpu"]["cores"], 8);
        let error = handle("reboot", &json!({})).expect_err("unknown");
        assert_eq!(error.kind(), anth_contract::ErrorKind::NotFound);
    }
}
