//! Renders a snapshot as a bordered table for the CLI.
//!
//! Deliberately small: v0.1.0's `render.rs` grew to 973 lines because each
//! feature contributed a bespoke layout. The shared piece here is the table in
//! [`crate::table`], and a feature supplies rows.

use anth_wire::HostInfo;

use crate::table::Table;

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

/// Renders a host snapshot as a bordered table.
pub fn render_host_info(info: &HostInfo) -> String {
    Table::new()
        .separate_rows(true)
        .rows(
            host_rows(info)
                .into_iter()
                .map(|(label, value)| [label.to_string(), value]),
        )
        .render()
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

    let rows = plugins.iter().map(|plugin| {
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
    });

    let mut out = Table::new()
        .with_headers(["ID", "ALIAS", "NAME", "VERSION", "STATE", "ANCHORS"])
        .rows(rows)
        .render();

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
