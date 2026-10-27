//! Human output for the CLI.
//!
//! The shape mirrors the web card: grouped sections, no dense field labels.

use std::fmt::Write as _;

use anth_wire::{FileEntry, FirewallSnapshot, HostInfo, NetworkSnapshot, Page, ProcessInfo};

/// Formats a host snapshot as grouped sections.
pub fn host_info(info: &HostInfo) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "Anthracite Panel · {}", value(&info.hostname));
    let _ = writeln!(
        out,
        "{} {} · kernel {} · {} · up {}",
        value(&info.os.name),
        value(&info.os.version),
        value(&info.os.kernel),
        value(&info.os.arch),
        format_uptime(info.uptime_seconds)
    );

    let _ = writeln!(out);
    let _ = writeln!(out, "CPU");
    let _ = writeln!(out, "  {}", value(&info.cpu_model));
    let _ = writeln!(
        out,
        "  {} logical cores · load {} {} {}",
        info.logical_cores,
        format_load(info.load.one),
        format_load(info.load.five),
        format_load(info.load.fifteen)
    );

    let memory = info.memory;
    let used = memory.used_kib();
    let percent = usage_percent(used, memory.total_kib);
    let _ = writeln!(out);
    let _ = writeln!(out, "Memory");
    let _ = writeln!(
        out,
        "  {} / {} used ({percent:.1}%) · {} available",
        format_kib(used),
        format_kib(memory.total_kib),
        format_kib(memory.available_kib)
    );

    let _ = writeln!(out);
    let _ = writeln!(out, "Swap");
    if memory.swap_total_kib == 0 {
        let _ = writeln!(out, "  not configured");
    } else {
        let used = memory.swap_used_kib();
        let _ = writeln!(
            out,
            "  {} / {} used ({:.1}%) · {} free",
            format_kib(used),
            format_kib(memory.swap_total_kib),
            usage_percent(used, memory.swap_total_kib),
            format_kib(memory.swap_free_kib)
        );
    }

    out.trim_end().to_string()
}

fn value(text: &str) -> &str {
    if text.trim().is_empty() || text == "unknown" {
        "unknown"
    } else {
        text
    }
}

/// Formats a duration in seconds as `3d 4h 5m 6s`.
pub fn format_uptime(seconds: u64) -> String {
    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3_600;
    let minutes = (seconds % 3_600) / 60;
    let secs = seconds % 60;
    let mut parts = Vec::new();
    if days > 0 {
        parts.push(format!("{days}d"));
    }
    if hours > 0 {
        parts.push(format!("{hours}h"));
    }
    if minutes > 0 {
        parts.push(format!("{minutes}m"));
    }
    if secs > 0 || parts.is_empty() {
        parts.push(format!("{secs}s"));
    }
    parts.join(" ")
}

/// Formats a kibibyte count with `KiB`, `MiB` or `GiB`.
pub fn format_kib(kib: u64) -> String {
    let mib = kib as f64 / 1024.0;
    if mib >= 1024.0 {
        format!("{:.1} GiB", mib / 1024.0)
    } else if mib >= 1.0 {
        format!("{mib:.1} MiB")
    } else {
        format!("{kib} KiB")
    }
}

pub fn format_load(value: f64) -> String {
    format!("{value:.2}")
}

fn usage_percent(used: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        (used as f64 / total as f64 * 100.0).clamp(0.0, 100.0)
    }
}

/// Formats one page of a directory listing.
pub fn file_list(page: &Page<FileEntry>) -> String {
    if page.items.is_empty() {
        return "(empty)".to_string();
    }
    let mut out = String::new();
    let _ = writeln!(out, "{:<5} {:>10}  {:<6} NAME", "KIND", "SIZE", "AGE");
    for entry in &page.items {
        let kind = match entry.kind.as_str() {
            "symlink" => "link",
            other => other,
        };
        let size = if entry.kind == "file" {
            format_bytes(entry.size)
        } else {
            "-".to_string()
        };
        let _ = writeln!(
            out,
            "{kind:<5} {:>10}  {:<6} {}",
            size,
            format_age(entry.modified_ms),
            entry.name
        );
    }
    if let Some(next) = &page.next {
        let _ = writeln!(out, "next: {next}");
    }
    out.trim_end().to_string()
}

/// Formats one network snapshot: one row per address, counters on the first.
pub fn network(snapshot: &NetworkSnapshot) -> String {
    if snapshot.interfaces.is_empty() {
        return "(no interfaces)".to_string();
    }
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{:<15} {:<8} {:<8} {:>5}  {:<39} {:>10} {:>10}",
        "INTERFACE", "STATE", "KIND", "MTU", "ADDRESS", "RX", "TX"
    );
    for interface in &snapshot.interfaces {
        let mut addresses = interface.addresses.iter();
        let first = addresses
            .next()
            .map(|address| format!("{}/{}", address.address, address.prefix))
            .unwrap_or_else(|| "-".to_string());
        let _ = writeln!(
            out,
            "{:<15} {:<8} {:<8} {:>5}  {:<39} {:>10} {:>10}",
            safe(&interface.name),
            safe(&interface.state),
            safe(&interface.kind),
            interface.mtu,
            first,
            format_bytes(interface.rx_bytes),
            format_bytes(interface.tx_bytes),
        );
        for address in addresses {
            let _ = writeln!(
                out,
                "{:<15} {:<8} {:<8} {:>5}  {:<39}",
                "",
                "",
                "",
                "",
                format!("{}/{}", address.address, address.prefix),
            );
        }
    }
    if !snapshot.routes.is_empty() {
        let _ = writeln!(out);
        let _ = writeln!(out, "DEFAULT ROUTES");
        for route in &snapshot.routes {
            let _ = writeln!(
                out,
                "  {} via {} dev {}",
                route.family, route.gateway, route.interface
            );
        }
    }
    out.trim_end().to_string()
}

/// Formats the managed firewall chain. A missing table is a normal state: the
/// daemon has simply never created it.
pub fn firewall(snapshot: &FirewallSnapshot) -> String {
    if !snapshot.initialized {
        return "Firewall table is not initialized. Deny a port to create it.".to_string();
    }
    if snapshot.rules.is_empty() {
        return "Managed firewall chain is empty.".to_string();
    }
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{:>6} {:<6} {:<6} {:>5}  COMMENT",
        "HANDLE", "ACTION", "PROTO", "PORT"
    );
    for rule in &snapshot.rules {
        let _ = writeln!(
            out,
            "{:>6} {:<6} {:<6} {:>5}  {}",
            rule.handle,
            safe(&rule.action).to_uppercase(),
            rule.protocol
                .map(|protocol| protocol.as_str())
                .unwrap_or("-"),
            rule.port
                .map(|port| port.to_string())
                .unwrap_or_else(|| "-".to_string()),
            rule.comment
                .as_deref()
                .map(safe)
                .unwrap_or_else(|| "-".to_string()),
        );
    }
    out.trim_end().to_string()
}

/// Formats one page of the process table.
pub fn process_list(page: &Page<ProcessInfo>) -> String {
    if page.items.is_empty() {
        return "(no processes)".to_string();
    }
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{:>7} {:>7} {:<12} {:<5} {:>10} {:>9} {:>4} COMMAND",
        "PID", "PPID", "USER", "STATE", "TICKS", "MEM", "THR"
    );
    for process in &page.items {
        let _ = writeln!(
            out,
            "{:>7} {:>7} {:<12} {:<5} {:>10} {:>9} {:>4} {}",
            process.pid,
            process.ppid,
            truncate(&safe(&process.user), 12),
            safe(&process.state),
            process.cpu_ticks,
            format_kib(process.rss_kib),
            process.threads,
            safe(&process.command),
        );
    }
    if let Some(next) = &page.next {
        let _ = writeln!(out, "next: {next}");
    }
    out.trim_end().to_string()
}

/// Keeps a terminal escape embedded in a command line from reaching the
/// operator's terminal.
fn safe(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_control() {
                '?'
            } else {
                character
            }
        })
        .collect()
}

fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    text.chars()
        .take(width.saturating_sub(1))
        .collect::<String>()
        + "…"
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn format_age(modified_ms: u64) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    if modified_ms == 0 {
        return "-".to_string();
    }
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0);
    if now_ms <= modified_ms {
        return "now".to_string();
    }
    let seconds = (now_ms - modified_ms) / 1000;
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3_600 {
        format!("{}m", seconds / 60)
    } else if seconds < 86_400 {
        format!("{}h", seconds / 3_600)
    } else {
        format!("{}d", seconds / 86_400)
    }
}

#[cfg(test)]
mod tests {
    use anth_wire::{LoadAverage, MemoryInfo, OsInfo};

    use super::*;

    fn sample() -> HostInfo {
        HostInfo {
            hostname: "example".to_string(),
            os: OsInfo {
                name: "Debian GNU/Linux".to_string(),
                version: "13".to_string(),
                kernel: "6.12.0".to_string(),
                arch: "x86_64".to_string(),
            },
            uptime_seconds: 3 * 86_400 + 4 * 3_600 + 5 * 60 + 6,
            cpu_model: "Example CPU".to_string(),
            logical_cores: 8,
            load: LoadAverage {
                one: 0.12,
                five: 0.20,
                fifteen: 0.31,
            },
            memory: MemoryInfo {
                total_kib: 8 * 1024 * 1024,
                available_kib: 4 * 1024 * 1024,
                swap_total_kib: 0,
                swap_free_kib: 0,
            },
            collected_at_ms: 1,
        }
    }

    #[test]
    fn host_info_matches_the_grouped_shape() {
        let text = host_info(&sample());
        assert!(text.starts_with("Anthracite Panel · example\n"));
        assert!(text.contains("Debian GNU/Linux 13 · kernel 6.12.0 · x86_64 · up 3d 4h 5m 6s"));
        assert!(text.contains("CPU\n  Example CPU\n  8 logical cores · load 0.12 0.20 0.31"));
        assert!(text.contains("Memory\n  4.0 GiB / 8.0 GiB used (50.0%) · 4.0 GiB available"));
        assert!(text.contains("Swap\n  not configured"));
        assert!(!text.ends_with('\n'));
    }

    #[test]
    fn swap_is_rendered_when_configured() {
        let mut info = sample();
        info.memory.swap_total_kib = 2 * 1024 * 1024;
        info.memory.swap_free_kib = 1024 * 1024;
        let text = host_info(&info);
        assert!(text.contains("Swap\n  1.0 GiB / 2.0 GiB used (50.0%) · 1.0 GiB free"));
    }

    #[test]
    fn formats_uptime_and_kib() {
        assert_eq!(format_uptime(0), "0s");
        assert_eq!(format_uptime(59), "59s");
        assert_eq!(format_uptime(60), "1m");
        assert_eq!(format_uptime(3_661), "1h 1m 1s");
        assert_eq!(format_kib(0), "0 KiB");
        assert_eq!(format_kib(2048), "2.0 MiB");
        assert_eq!(format_kib(2 * 1024 * 1024), "2.0 GiB");
    }

    #[test]
    fn a_process_list_has_a_header_and_neutralizes_control_characters() {
        let page = Page::with_next(
            vec![ProcessInfo {
                pid: 42,
                ppid: 1,
                uid: 0,
                user: "root".to_string(),
                name: "sh".to_string(),
                command: "sh -c \u{1b}[31mred".to_string(),
                state: "S".to_string(),
                cpu_ticks: 12,
                rss_kib: 2048,
                threads: 1,
                start_ticks: 99,
            }],
            "1",
        );
        let text = process_list(&page);
        assert!(text.starts_with("    PID"));
        assert!(text.contains("TICKS"));
        assert!(text.contains("2.0 MiB"));
        assert!(text.contains("sh -c ?[31mred"));
        assert!(!text.contains('\u{1b}'));
        assert!(text.ends_with("next: 1"));
    }

    #[test]
    fn a_file_list_has_a_header_rows_and_a_next_cursor() {
        let page = Page::with_next(
            vec![
                FileEntry {
                    name: "dir".to_string(),
                    kind: "dir".to_string(),
                    size: 0,
                    modified_ms: 0,
                },
                FileEntry {
                    name: "note.txt".to_string(),
                    kind: "file".to_string(),
                    size: 2048,
                    modified_ms: 0,
                },
            ],
            "note.txt",
        );
        let text = file_list(&page);
        assert!(text.starts_with("KIND"));
        assert!(text.contains("dir"));
        assert!(text.contains("2.0 KiB"));
        assert!(text.contains("next: note.txt"));
    }
}
