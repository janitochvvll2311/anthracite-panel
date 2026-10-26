//! Pure parsers for the `/proc` and `os-release` formats.
//!
//! Each parser takes file contents and returns data, so it is testable without
//! a fake filesystem.

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
    while let Some(character) = chars.next() {
        if character != '\\' {
            out.push(character);
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

/// Extracts `NAME` and `VERSION_ID`, falling back to `VERSION`.
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
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| super::UNKNOWN.to_string());
    let version = version
        .or(fallback_version)
        .filter(|value| !value.is_empty())
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
pub fn parse_cpu_cores(text: &str) -> u32 {
    let count = text
        .lines()
        .filter(|line| {
            line.split_once(':')
                .is_some_and(|(name, _)| name.trim() == "processor")
        })
        .count();
    u32::try_from(count).unwrap_or(u32::MAX)
}

/// Converts `/proc/meminfo` into kibibytes.
pub fn parse_meminfo(text: &str) -> MemoryInfo {
    let mut memory = MemoryInfo::default();

    for line in text.lines() {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let mut parts = value.split_whitespace();
        let Some(amount) = parts.next().and_then(|value| value.parse::<u64>().ok()) else {
            continue;
        };
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
            .and_then(|field| field.parse::<f64>().ok())
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
        .and_then(|field| field.parse::<f64>().ok())
        .unwrap_or(0.0)
        .max(0.0) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_release_prefers_version_id() {
        let text = r#"
NAME="Debian GNU/Linux"
VERSION="13 (trixie)"
VERSION_ID="13"
"#;
        assert_eq!(
            parse_os_release(text),
            ("Debian GNU/Linux".to_string(), "13".to_string())
        );
    }

    #[test]
    fn os_release_falls_back_to_version() {
        let text = "NAME=\"Alpine Linux\"\nVERSION=\"3.24.3\"\n";
        assert_eq!(
            parse_os_release(text),
            ("Alpine Linux".to_string(), "3.24.3".to_string())
        );
    }

    #[test]
    fn os_release_unknown_when_empty() {
        assert_eq!(
            parse_os_release(""),
            ("unknown".to_string(), "unknown".to_string())
        );
    }

    #[test]
    fn os_release_unescapes_quoted_values() {
        let text = "NAME=\"A \\\"quoted\\\" $name\"\nVERSION_ID=\"1\"\n";
        assert_eq!(
            parse_os_release(text),
            ("A \"quoted\" $name".to_string(), "1".to_string())
        );
    }

    #[test]
    fn cpu_model_reads_x86_and_arm_keys() {
        assert_eq!(
            parse_cpu_model("model name : Example CPU\n"),
            Some("Example CPU".to_string())
        );
        assert_eq!(
            parse_cpu_model("Hardware : ARMv8 Processor rev 1\n"),
            Some("ARMv8 Processor rev 1".to_string())
        );
        assert_eq!(parse_cpu_model("processor : 0\n"), None);
    }

    #[test]
    fn cpu_cores_counts_processor_lines() {
        let text = "processor : 0\nmodel name : x\n\nprocessor : 1\nmodel name : x\n";
        assert_eq!(parse_cpu_cores(text), 2);
        assert_eq!(parse_cpu_cores(""), 0);
    }

    #[test]
    fn meminfo_reads_kib_fields() {
        let text = "MemTotal:       16384000 kB\nMemAvailable:    8192000 kB\nSwapTotal:       2097152 kB\nSwapFree:        1048576 kB\n";
        let memory = parse_meminfo(text);
        assert_eq!(memory.total_kib, 16_384_000);
        assert_eq!(memory.available_kib, 8_192_000);
        assert_eq!(memory.swap_total_kib, 2_097_152);
        assert_eq!(memory.swap_free_kib, 1_048_576);
        assert_eq!(memory.used_kib(), 8_192_000);
        assert_eq!(memory.swap_used_kib(), 1_048_576);
    }

    #[test]
    fn meminfo_accepts_other_units_and_ignores_bad_lines() {
        let text = "MemTotal: 2 mB\nSwapTotal: 1 gB\nBogus: nope\n";
        let memory = parse_meminfo(text);
        assert_eq!(memory.total_kib, 2048);
        assert_eq!(memory.swap_total_kib, 1_048_576);
    }

    #[test]
    fn loadavg_reads_three_values() {
        let load = parse_loadavg("0.12 0.20 0.31 1/234 5678\n");
        assert_eq!(load.one, 0.12);
        assert_eq!(load.five, 0.20);
        assert_eq!(load.fifteen, 0.31);
    }

    #[test]
    fn loadavg_missing_fields_are_zero() {
        let load = parse_loadavg("0.5\n");
        assert_eq!(load.one, 0.5);
        assert_eq!(load.five, 0.0);
        assert_eq!(load.fifteen, 0.0);
    }

    #[test]
    fn uptime_reads_whole_seconds() {
        assert_eq!(parse_uptime("1234.56 999.00\n"), 1234);
        assert_eq!(parse_uptime("nonsense"), 0);
    }
}
