//! Pure parsers for the `/proc` and `os-release` formats.
//!
//! Each one takes file contents and returns data, which is what makes the
//! parsing testable without a fake filesystem. Ported from v0.1.0 unchanged in
//! behaviour: these were already correct and heavily covered.

use anth_wire::{LoadAverage, MemoryInfo};

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
            // The format permits exactly these escapes.
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

/// Extracts `NAME` and `VERSION_ID`, falling back to `VERSION`.
///
/// Returns `(unknown, unknown)` when neither is present.
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
        .unwrap_or_else(|| super::UNKNOWN.to_string());
    let version = version
        .or(fallback_version)
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| super::UNKNOWN.to_string());
    (name, version)
}

/// Reads the model of the first processor.
///
/// ARM kernels expose `Hardware` or `Processor` instead of x86's `model name`.
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

/// Counts the `processor` records, one per logical core.
pub fn parse_cpu_cores(text: &str) -> usize {
    text.lines()
        .filter(|line| {
            line.split_once(':')
                .is_some_and(|(name, _)| name.trim() == "processor")
        })
        .count()
}

/// Converts `/proc/meminfo` into kibibytes.
///
/// The kernel always reports `kB`, but other units are accepted defensively.
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
        // A bare number is already in kB, which is the only unit Linux uses here.
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

/// Reads the three load averages; `0.0` when the kernel reports fewer.
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
