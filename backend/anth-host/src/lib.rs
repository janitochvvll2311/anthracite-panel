//! Host inspection for Linux, built on `/proc` and `/etc/os-release`.
//!
//! Ported from v0.1.0 because it was already the right shape: standard library
//! only (no `sysinfo`-style dependency, no `unsafe`, so the static musl build
//! stays trivial), and every parser is a pure function over file contents so it
//! can be tested against fixtures. [`collect_from`] takes a filesystem root for
//! exactly that reason.
//!
//! Per-process inspection is deliberately out of scope; this describes the host.

mod parse;

pub use parse::{
    parse_cpu_cores, parse_cpu_model, parse_loadavg, parse_meminfo, parse_os_release, parse_uptime,
};

use std::path::Path;

use anth_wire::{HostInfo, OsInfo};

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
///
/// A missing or non-UTF-8 source degrades to `None` rather than failing the
/// whole snapshot: the panel should still say what it can.
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

/// Collects a snapshot of the host the daemon runs on.
pub fn collect() -> HostInfo {
    let (os_name, os_version) = match read_first(&[OS_RELEASE, OS_RELEASE_FALLBACK]) {
        Some(text) => parse_os_release(&text),
        None => (UNKNOWN.to_string(), UNKNOWN.to_string()),
    };

    let cpuinfo = read_trimmed(CPUINFO).unwrap_or_default();
    let cores = match parse_cpu_cores(&cpuinfo) {
        0 => std::thread::available_parallelism().map_or(0, |n| n.get()),
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
    }
}

/// Reads a snapshot from an alternative filesystem root.
///
/// This is how the parsers are exercised against fixture files instead of the
/// machine running the tests.
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
    }
}

#[cfg(test)]
mod tests;
