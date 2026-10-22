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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_release_reads_name_and_version_id() {
        let (name, version) = parse_os_release("NAME=Alpine Linux\nVERSION_ID=3.20\n");
        assert_eq!(name, "Alpine Linux");
        assert_eq!(version, "3.20");
    }

    #[test]
    fn os_release_falls_back_to_version_when_version_id_is_absent() {
        let (_, version) = parse_os_release("NAME=Debian\nVERSION=\"12 (bookworm)\"\n");
        assert_eq!(version, "12 (bookworm)");
    }

    #[test]
    fn os_release_prefers_version_id_over_version() {
        let (_, version) = parse_os_release("NAME=X\nVERSION_ID=1\nVERSION=ignored\n");
        assert_eq!(version, "1");
    }

    #[test]
    fn os_release_resolves_the_permitted_escapes() {
        let (name, _) = parse_os_release(r#"NAME="a\"b\\c\$d\`e""#);
        assert_eq!(name, r#"a"b\c$d`e"#);
    }

    #[test]
    fn os_release_keeps_an_unknown_escape_literal() {
        let (name, _) = parse_os_release(r#"NAME="a\qb""#);
        assert_eq!(name, r"a\qb");
    }

    #[test]
    fn os_release_ignores_comments_and_malformed_lines() {
        let (name, version) = parse_os_release("# NAME=Nope\nnonsense\nNAME=Real\n");
        assert_eq!(name, "Real");
        assert_eq!(version, super::super::UNKNOWN);
    }

    #[test]
    fn os_release_answers_unknown_when_nothing_is_present() {
        let (name, version) = parse_os_release("");
        assert_eq!(name, super::super::UNKNOWN);
        assert_eq!(version, super::super::UNKNOWN);
    }

    #[test]
    fn os_release_treats_an_empty_value_as_absent() {
        let (name, _) = parse_os_release("NAME=\n");
        assert_eq!(name, super::super::UNKNOWN);
    }

    #[test]
    fn cpu_model_reads_the_x86_key() {
        let text = "processor\t: 0\nmodel name\t: Intel Example\n";
        assert_eq!(parse_cpu_model(text).as_deref(), Some("Intel Example"));
    }

    #[test]
    fn cpu_model_reads_the_arm_keys() {
        assert_eq!(
            parse_cpu_model("Hardware\t: BCM2711\n").as_deref(),
            Some("BCM2711")
        );
        assert_eq!(
            parse_cpu_model("Processor\t: ARMv7\n").as_deref(),
            Some("ARMv7")
        );
    }

    #[test]
    fn cpu_model_is_none_when_absent_or_blank() {
        assert!(parse_cpu_model("processor\t: 0\n").is_none());
        assert!(parse_cpu_model("model name\t:   \n").is_none());
    }

    #[test]
    fn cpu_cores_counts_processor_records_only() {
        let text = "processor\t: 0\nmodel name\t: X\n\nprocessor\t: 1\n";
        assert_eq!(parse_cpu_cores(text), 2);
        assert_eq!(parse_cpu_cores("model name\t: X\n"), 0);
    }

    #[test]
    fn meminfo_reads_kibibytes() {
        let memory = parse_meminfo(
            "MemTotal:       1000 kB\nMemAvailable:    400 kB\n\
             SwapTotal:       200 kB\nSwapFree:        50 kB\n",
        );
        assert_eq!(memory.total_kib, 1000);
        assert_eq!(memory.available_kib, 400);
        assert_eq!(memory.swap_total_kib, 200);
        assert_eq!(memory.swap_free_kib, 50);
        assert_eq!(memory.used_kib(), 600);
        assert_eq!(memory.swap_used_kib(), 150);
    }

    #[test]
    fn meminfo_converts_the_larger_units() {
        let memory = parse_meminfo("MemTotal: 1 mB\nSwapTotal: 1 gB\n");
        assert_eq!(memory.total_kib, 1024);
        assert_eq!(memory.swap_total_kib, 1024 * 1024);
    }

    #[test]
    fn meminfo_skips_an_unrecognized_unit_instead_of_guessing() {
        let memory = parse_meminfo("MemTotal: 1 MiB\n");
        assert_eq!(memory.total_kib, 0);
    }

    #[test]
    fn meminfo_ignores_unrelated_keys() {
        let memory = parse_meminfo("Buffers: 10 kB\nCached: 20 kB\n");
        assert_eq!(memory, MemoryInfo::default());
    }

    #[test]
    fn loadavg_reads_three_fields_and_stops() {
        let load = parse_loadavg("0.5 0.25 0.125 2/300 4242\n");
        assert_eq!(load.one, 0.5);
        assert_eq!(load.five, 0.25);
        assert_eq!(load.fifteen, 0.125);
    }

    #[test]
    fn loadavg_zeroes_the_missing_fields() {
        let load = parse_loadavg("1.5\n");
        assert_eq!(load.one, 1.5);
        assert_eq!(load.five, 0.0);
        assert_eq!(load.fifteen, 0.0);
    }

    #[test]
    fn uptime_truncates_to_whole_seconds() {
        assert_eq!(parse_uptime("1234.99 999.0\n"), 1234);
        assert_eq!(parse_uptime(""), 0);
        assert_eq!(parse_uptime("not a number"), 0);
    }
}
