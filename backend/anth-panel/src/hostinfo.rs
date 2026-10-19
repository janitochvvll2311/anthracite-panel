//! Host inspection for Linux, built on `/proc`, `/sys` and `/etc/os-release` so
//! that no external crate or `unsafe` is required. Per-process inspection is
//! intentionally out of scope.

use std::path::Path;

use anth_proto::{HostInfo, LoadAverage, MemoryInfo, OsInfo};

use crate::text::read_trimmed;

const HOSTNAME: &str = "/proc/sys/kernel/hostname";
const OS_RELEASE: &str = "/etc/os-release";
const OS_RELEASE_FALLBACK: &str = "/usr/lib/os-release";
const OSRELEASE: &str = "/proc/sys/kernel/osrelease";
const UPTIME: &str = "/proc/uptime";
const CPUINFO: &str = "/proc/cpuinfo";
const LOADAVG: &str = "/proc/loadavg";
const MEMINFO: &str = "/proc/meminfo";

const UNKNOWN: &str = "unknown";

fn read_first(paths: &[&str]) -> Option<String> {
    paths.iter().find_map(|path| read_trimmed(*path))
}

/// Strips the optional surrounding quotes of an `os-release` value and resolves
/// the `\"`, `\\`, `\$` and `` \` `` escapes the format allows.
fn unquote(value: &str) -> String {
    let value = value.trim();
    let Some(inner) = value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
    else {
        return value.to_string();
    };

    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some(escaped @ ('"' | '\\' | '$' | '`')) => out.push(escaped),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Extracts `NAME` and `VERSION_ID` (falling back to `VERSION`) from the
/// `os-release` format. Returns `(unknown, unknown)` when neither is present.
pub fn parse_os_release(text: &str) -> (String, String) {
    let (mut name, mut version, mut fallback_version) = (None, None, None);

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = unquote(value);
        match key.trim() {
            "NAME" => name = Some(value),
            "VERSION_ID" => version = Some(value),
            "VERSION" => fallback_version = Some(value),
            _ => {}
        }
    }

    let name = name
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| UNKNOWN.to_string());
    let version = version
        .or(fallback_version)
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| UNKNOWN.to_string());
    (name, version)
}

/// Reads the model of the first processor described in `/proc/cpuinfo`. ARM
/// kernels expose `Hardware` or `Processor` instead of x86's `model name`.
pub fn parse_cpu_model(text: &str) -> Option<String> {
    for key in ["model name", "Hardware", "Processor", "cpu model"] {
        for line in text.lines() {
            let Some((name, value)) = line.split_once(':') else {
                continue;
            };
            if name.trim() == key {
                let value = value.trim();
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}

/// Counts the `processor` records in `/proc/cpuinfo`, one per logical core.
pub fn parse_cpu_cores(text: &str) -> usize {
    text.lines()
        .filter(|line| {
            line.split_once(':')
                .is_some_and(|(name, _)| name.trim() == "processor")
        })
        .count()
}

/// Converts a human-readable size from `/proc/meminfo` into kibibytes. The
/// kernel always reports `kB`, but other units are accepted defensively.
pub fn parse_meminfo(text: &str) -> MemoryInfo {
    let mut memory = MemoryInfo::default();

    for line in text.lines() {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let mut parts = value.split_whitespace();
        let Some(amount) = parts.next().and_then(|v| v.parse::<u64>().ok()) else {
            continue;
        };
        // Bare numbers are already in kB, which is the only unit Linux uses here.
        let kib = match parts.next() {
            None | Some("kB") => amount,
            Some("mB") => amount.saturating_mul(1024),
            Some("gB") => amount.saturating_mul(1024 * 1024),
            Some(_) => continue,
        };

        match name.trim() {
            "MemTotal" => memory.total_kib = kib,
            "MemAvailable" => memory.available_kib = kib,
            "SwapTotal" => memory.swap_total_kib = kib,
            "SwapFree" => memory.swap_free_kib = kib,
            _ => {}
        }
    }

    memory
}

/// Reads the three load averages from `/proc/loadavg`. They are `0.0` when the
/// kernel reports fewer than three fields.
pub fn parse_loadavg(text: &str) -> LoadAverage {
    let mut fields = text.split_whitespace();
    let mut next = || {
        fields
            .next()
            .and_then(|f| f.parse::<f64>().ok())
            .unwrap_or(0.0)
    };
    LoadAverage {
        one: next(),
        five: next(),
        fifteen: next(),
    }
}

/// Reads whole seconds of uptime from the first field of `/proc/uptime`.
pub fn parse_uptime(text: &str) -> u64 {
    text.split_whitespace()
        .next()
        .and_then(|f| f.parse::<f64>().ok())
        .unwrap_or(0.0)
        .max(0.0) as u64
}

/// Collects a snapshot of the host, degrading each field to `unknown`/zero when
/// the underlying file is unreadable rather than failing the whole request.
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

/// Reads a snapshot from an alternative filesystem root, used by tests to
/// exercise the parsing paths against fixture files.
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
