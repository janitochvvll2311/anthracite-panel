//! Host inspection for Linux, built on `/proc` and `/etc/os-release`.
//!
//! Standard library only: no `sysinfo`-style dependency and no `unsafe`. Every
//! parser is a pure function over file contents, and [`collect_from`] takes a
//! filesystem root so tests can use fixtures.

mod parse;

pub use parse::{
    parse_cpu_cores, parse_cpu_model, parse_loadavg, parse_meminfo, parse_os_release, parse_uptime,
};

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anth_wire::{HostInfo, OsInfo};

pub mod backend;
pub mod demo;
pub mod files;
pub mod firewall;
pub mod network;
pub mod process;
pub mod terminal;
pub mod volumes;

const HOSTNAME: &str = "/proc/sys/kernel/hostname";
const OS_RELEASE: &str = "/etc/os-release";
const OS_RELEASE_FALLBACK: &str = "/usr/lib/os-release";
const OSRELEASE: &str = "/proc/sys/kernel/osrelease";
const UPTIME: &str = "/proc/uptime";
const CPUINFO: &str = "/proc/cpuinfo";
const LOADAVG: &str = "/proc/loadavg";
const MEMINFO: &str = "/proc/meminfo";

pub const UNKNOWN: &str = "unknown";

/// Reads the first readable path, returning `None` if none is.
fn read_trimmed(path: impl AsRef<Path>) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let text = String::from_utf8(bytes).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn read_first(paths: &[&str]) -> Option<String> {
    paths.iter().find_map(|path| read_trimmed(*path))
}

/// Unix epoch milliseconds, or `0` if the clock is before 1970.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

/// Collects a snapshot of the host the daemon runs on.
pub fn collect() -> HostInfo {
    let (os_name, os_version) = match read_first(&[OS_RELEASE, OS_RELEASE_FALLBACK]) {
        Some(text) => parse_os_release(&text),
        None => (UNKNOWN.to_string(), UNKNOWN.to_string()),
    };

    let cpuinfo = read_trimmed(CPUINFO).unwrap_or_default();
    let cores = match parse_cpu_cores(&cpuinfo) {
        0 => std::thread::available_parallelism().map_or(0, |n| n.get() as u32),
        count => count,
    };

    HostInfo {
        hostname: read_first(&[HOSTNAME])
            .or_else(|| std::env::var("HOSTNAME").ok())
            .unwrap_or_else(|| UNKNOWN.to_string()),
        os: OsInfo {
            name: os_name,
            version: os_version,
            kernel: read_first(&[OSRELEASE]).unwrap_or_else(|| UNKNOWN.to_string()),
            arch: std::env::consts::ARCH.to_string(),
        },
        uptime_seconds: read_trimmed(UPTIME)
            .map(|text| parse_uptime(&text))
            .unwrap_or(0),
        cpu_model: parse_cpu_model(&cpuinfo).unwrap_or_else(|| UNKNOWN.to_string()),
        logical_cores: cores,
        load: read_trimmed(LOADAVG)
            .map(|text| parse_loadavg(&text))
            .unwrap_or_default(),
        memory: read_trimmed(MEMINFO)
            .map(|text| parse_meminfo(&text))
            .unwrap_or_default(),
        collected_at_ms: now_ms(),
    }
}

/// Reads a snapshot from an alternative filesystem root.
pub fn collect_from(root: impl AsRef<Path>) -> HostInfo {
    let root = root.as_ref();
    let read = |relative: &str| read_trimmed(root.join(relative));

    let (os_name, os_version) = read("etc/os-release")
        .map(|text| parse_os_release(&text))
        .unwrap_or_else(|| (UNKNOWN.to_string(), UNKNOWN.to_string()));
    let cpuinfo = read("proc/cpuinfo").unwrap_or_default();

    HostInfo {
        hostname: read("proc/sys/kernel/hostname").unwrap_or_else(|| UNKNOWN.to_string()),
        os: OsInfo {
            name: os_name,
            version: os_version,
            kernel: read("proc/sys/kernel/osrelease").unwrap_or_else(|| UNKNOWN.to_string()),
            arch: std::env::consts::ARCH.to_string(),
        },
        uptime_seconds: read("proc/uptime")
            .map(|text| parse_uptime(&text))
            .unwrap_or(0),
        cpu_model: parse_cpu_model(&cpuinfo).unwrap_or_else(|| UNKNOWN.to_string()),
        logical_cores: parse_cpu_cores(&cpuinfo),
        load: read("proc/loadavg")
            .map(|text| parse_loadavg(&text))
            .unwrap_or_default(),
        memory: read("proc/meminfo")
            .map(|text| parse_meminfo(&text))
            .unwrap_or_default(),
        collected_at_ms: now_ms(),
    }
}

#[cfg(test)]
mod tests;
