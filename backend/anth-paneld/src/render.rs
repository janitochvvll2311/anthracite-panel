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

/// The plugin list, aligned, with the reasons a plugin is disabled listed after
/// it.
///
/// A disabled plugin stays visible on purpose: one that vanished from the list
/// would look like it was never installed, and the operator would have nothing
/// to fix. The reason goes on its own line rather than in the table because a
/// digest mismatch is a sentence, and a sentence in a column pushes every other
/// column off the screen.
pub fn render_plugin_list(plugins: &[anth_wire::PluginSummary]) -> String {
    if plugins.is_empty() {
        return "no plugins installed".to_string();
    }

    let rows: Vec<[String; 6]> = plugins
        .iter()
        .map(|plugin| {
            [
                plugin.id.clone(),
                plugin.alias.clone().unwrap_or_default(),
                plugin.name.clone(),
                plugin.version.clone(),
                match plugin.disabled_reason {
                    None => "enabled".to_string(),
                    Some(_) => "disabled".to_string(),
                },
                plugin.anchors.join(", "),
            ]
        })
        .collect();

    let header = ["ID", "ALIAS", "NAME", "VERSION", "STATE", "ANCHORS"];
    let mut widths = [0usize; 6];
    for (index, column) in header.iter().enumerate() {
        widths[index] = width(column).max(
            rows.iter()
                .map(|row| width(&row[index]))
                .max()
                .unwrap_or_default(),
        );
    }

    let line = |cells: [&str; 6]| {
        let padded: Vec<String> = cells
            .iter()
            .enumerate()
            .map(|(index, cell)| {
                let padding = widths[index].saturating_sub(width(cell));
                format!("{cell}{}", " ".repeat(padding))
            })
            .collect();
        format!("  {}", padded.join("  ")).trim_end().to_string()
    };

    let mut out = String::new();
    out.push_str(&line(header));
    for row in &rows {
        out.push('\n');
        out.push_str(&line([
            &row[0], &row[1], &row[2], &row[3], &row[4], &row[5],
        ]));
    }

    let disabled: Vec<(&str, &str)> = plugins
        .iter()
        .filter_map(|plugin| {
            plugin
                .disabled_reason
                .as_deref()
                .map(|reason| (plugin.id.as_str(), reason))
        })
        .collect();
    if !disabled.is_empty() {
        out.push('\n');
        for (id, reason) in disabled {
            out.push_str(&format!("\n  {id}: {reason}"));
        }
    }
    out
}

/// A plugin's value as text: the string it answered, or its JSON.
///
/// A plugin usually answers a string, and printing it with the wire's quotes
/// would leak the encoding into the terminal.
pub fn render_plugin_reply(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Null => String::new(),
        other => serde_json::to_string_pretty(other).unwrap_or_else(|_| other.to_string()),
    }
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

    /// The alias is an accepted name, so a list that hid it would leave a caller
    /// guessing which short handles exist. A plugin without one leaves the
    /// column blank rather than shifting the row.
    #[test]
    fn the_plugin_list_shows_the_alias() {
        let rendered = render_plugin_list(&[
            anth_wire::PluginSummary {
                id: "org.test.aliased".into(),
                alias: Some("short".into()),
                name: "Aliased".into(),
                version: "0.2.0".into(),
                anchors: vec!["GET /greeting".into()],
                disabled_reason: None,
            },
            anth_wire::PluginSummary {
                id: "org.test.plain".into(),
                alias: None,
                name: "Plain".into(),
                version: "0.2.0".into(),
                anchors: vec![],
                disabled_reason: Some("the bundle was rewritten".into()),
            },
        ]);

        assert!(rendered.contains("ALIAS"));
        assert!(rendered.contains("short"));
        // The second plugin has no alias, so its cell is blank: the state and
        // anchors still line up under their headers.
        assert!(rendered.contains("disabled"));
        assert!(rendered.contains("the bundle was rewritten"));
        let lines: Vec<&str> = rendered.lines().collect();
        assert_eq!(lines[0].split_whitespace().next(), Some("ID"));
        assert!(lines.iter().any(|line| line.contains("Plain")));
    }

    #[test]
    fn an_empty_plugin_list_says_so() {
        assert_eq!(render_plugin_list(&[]), "no plugins installed");
    }
}
