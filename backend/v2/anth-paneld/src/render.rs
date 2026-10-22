//! Renders a snapshot as a bordered table for the CLI.
//!
//! Deliberately small: v0.1.0's `render.rs` grew to 973 lines because each
//! feature contributed a bespoke layout. The shared piece here is the box
//! drawing, and a feature supplies rows.
//!
//! Pads by character count, not byte count, so multi-byte text stays aligned.

use anth_wire::HostInfo;

/// Formats a kibibyte count with the largest unit that keeps it readable.
pub fn format_kib(kib: u64) -> String {
    const UNITS: [(&str, u64); 4] = [("GiB", 1024 * 1024), ("MiB", 1024), ("KiB", 1), ("B", 0)];
    if kib == 0 {
        return "0 KiB".to_string();
    }
    for (unit, divisor) in UNITS {
        if divisor == 0 {
            continue;
        }
        if kib >= divisor {
            let value = kib as f64 / divisor as f64;
            return if divisor == 1 {
                format!("{kib} KiB")
            } else {
                format!("{value:.1} {unit}")
            };
        }
    }
    format!("{kib} KiB")
}

/// Formats an uptime with only its non-zero components.
pub fn format_uptime(seconds: u64) -> String {
    if seconds == 0 {
        return "0s".to_string();
    }
    let (days, rest) = (seconds / 86_400, seconds % 86_400);
    let (hours, rest) = (rest / 3_600, rest % 3_600);
    let (minutes, secs) = (rest / 60, rest % 60);

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
    if secs > 0 {
        parts.push(format!("{secs}s"));
    }
    parts.join(" ")
}

fn host_rows(info: &HostInfo) -> Vec<(&'static str, String)> {
    vec![
        ("Hostname", info.hostname.clone()),
        ("OS", info.os.name.clone()),
        ("Version", info.os.version.clone()),
        ("Kernel", info.os.kernel.clone()),
        ("Architecture", info.os.arch.clone()),
        ("CPU", info.cpu_model.clone()),
        ("Cores", info.logical_cores.to_string()),
        ("Uptime", format_uptime(info.uptime_seconds)),
        (
            "Load",
            format!(
                "{:.2} {:.2} {:.2}",
                info.load.one, info.load.five, info.load.fifteen
            ),
        ),
        (
            "Memory",
            format!(
                "{} used of {}",
                format_kib(info.memory.used_kib()),
                format_kib(info.memory.total_kib)
            ),
        ),
        (
            "Swap",
            format!(
                "{} used of {}",
                format_kib(info.memory.swap_used_kib()),
                format_kib(info.memory.swap_total_kib)
            ),
        ),
    ]
}

/// Width in characters, not bytes.
fn width(text: &str) -> usize {
    text.chars().count()
}

/// Renders a host snapshot as a bordered table.
pub fn render_host_info(info: &HostInfo) -> String {
    let rows = host_rows(info);
    let label_width = rows
        .iter()
        .map(|(label, _)| width(label))
        .max()
        .unwrap_or(0);
    let value_width = rows
        .iter()
        .map(|(_, value)| width(value))
        .max()
        .unwrap_or(0);

    let inner = label_width + 2 + value_width;
    let rule = "-".repeat(inner + 2);
    let mut out = String::new();

    out.push_str(&format!("+{rule}+\n"));
    for (label, value) in rows {
        let label_pad = label_width - width(label);
        let value_pad = value_width - width(&value);
        out.push_str(&format!(
            "| {label}{}  {value}{} |\n",
            " ".repeat(label_pad),
            " ".repeat(value_pad)
        ));
    }
    out.push_str(&format!("+{rule}+"));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use anth_wire::{HostInfo, LoadAverage, MemoryInfo, OsInfo};

    fn info() -> HostInfo {
        HostInfo {
            hostname: "anth".into(),
            os: OsInfo {
                name: "Alpine Linux".into(),
                version: "3.20".into(),
                kernel: "6.6.0".into(),
                arch: "x86_64".into(),
            },
            uptime_seconds: 90_061,
            cpu_model: "Example CPU".into(),
            logical_cores: 4,
            load: LoadAverage {
                one: 0.5,
                five: 0.25,
                fifteen: 0.125,
            },
            memory: MemoryInfo {
                total_kib: 2_048,
                available_kib: 1_024,
                swap_total_kib: 0,
                swap_free_kib: 0,
            },
        }
    }

    #[test]
    fn uptime_keeps_only_its_non_zero_components() {
        assert_eq!(format_uptime(0), "0s");
        assert_eq!(format_uptime(59), "59s");
        assert_eq!(format_uptime(3_600), "1h");
        assert_eq!(format_uptime(90_061), "1d 1h 1m 1s");
    }

    #[test]
    fn kib_uses_the_largest_readable_unit() {
        assert_eq!(format_kib(0), "0 KiB");
        assert_eq!(format_kib(512), "512 KiB");
        assert_eq!(format_kib(2_048), "2.0 MiB");
        assert_eq!(format_kib(8_388_608), "8.0 GiB");
    }

    #[test]
    fn the_table_is_a_closed_box_with_one_row_per_field() {
        let rendered = render_host_info(&info());
        let lines: Vec<&str> = rendered.lines().collect();

        assert!(lines[0].starts_with('+') && lines[0].ends_with('+'));
        assert!(lines.last().unwrap().starts_with('+'));
        for line in &lines[1..lines.len() - 1] {
            assert!(line.starts_with("| ") && line.ends_with(" |"), "{line}");
        }
        assert_eq!(lines.len(), 11 + 2, "eleven fields plus two rules");
    }

    #[test]
    fn every_line_has_the_same_character_width() {
        let rendered = render_host_info(&info());
        let widths: Vec<usize> = rendered.lines().map(width).collect();
        assert!(
            widths.windows(2).all(|w| w[0] == w[1]),
            "ragged table: {widths:?}"
        );
    }

    #[test]
    fn multi_byte_values_do_not_shift_the_border() {
        let mut snapshot = info();
        snapshot.hostname = "anfitrión".into();
        snapshot.cpu_model = "处理器".into();
        let rendered = render_host_info(&snapshot);
        let widths: Vec<usize> = rendered.lines().map(width).collect();
        assert!(
            widths.windows(2).all(|w| w[0] == w[1]),
            "multi-byte text broke the alignment: {widths:?}"
        );
    }

    #[test]
    fn the_values_appear_in_the_table() {
        let rendered = render_host_info(&info());
        assert!(rendered.contains("Alpine Linux"));
        assert!(rendered.contains("1d 1h 1m 1s"));
        assert!(rendered.contains("1.0 MiB used of 2.0 MiB"));
        assert!(rendered.contains("0 KiB used of 0 KiB"));
    }
}
