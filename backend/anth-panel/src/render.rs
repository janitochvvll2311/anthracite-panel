//! Rendering of host information as a bordered text table.

use anth_proto::HostInfo;

const TOP_LEFT: char = '┌';
const TOP_RIGHT: char = '┐';
const BOTTOM_LEFT: char = '└';
const BOTTOM_RIGHT: char = '┘';
const HORIZONTAL: char = '─';
const VERTICAL: char = '│';
const TEE_DOWN: char = '┬';
const TEE_UP: char = '┴';
const CROSS: char = '┼';
const TEE_RIGHT: char = '├';
const TEE_LEFT: char = '┤';

/// A `Row::Separator` draws a horizontal rule between groups of rows.
#[derive(Debug, Clone)]
enum Row {
    Cells(Vec<String>),
    Separator,
}

#[derive(Debug, Clone)]
pub struct Table {
    headers: Vec<String>,
    rows: Vec<Row>,
}

impl Table {
    pub fn new(headers: Vec<String>) -> Self {
        Self {
            headers,
            rows: Vec::new(),
        }
    }

    pub fn push(&mut self, cells: Vec<String>) -> &mut Self {
        self.rows.push(Row::Cells(cells));
        self
    }

    pub fn separator(&mut self) -> &mut Self {
        self.rows.push(Row::Separator);
        self
    }

    /// Renders the table with box-drawing borders. Column widths are driven by
    /// the widest cell, measured in characters so multi-byte text stays aligned.
    pub fn render(&self) -> String {
        let columns = self.headers.len();
        let mut widths: Vec<usize> = self.headers.iter().map(|h| width(h)).collect();

        for row in &self.rows {
            let Row::Cells(cells) = row else {
                continue;
            };
            for (index, cell) in cells.iter().enumerate() {
                if index < columns {
                    widths[index] = widths[index].max(width(cell));
                }
            }
        }

        let mut out = String::new();
        out.push_str(&self.border(TOP_LEFT, TEE_DOWN, TOP_RIGHT, &widths));
        out.push('\n');
        out.push_str(&self.line(&self.headers, &widths));
        out.push('\n');
        out.push_str(&self.border(TEE_RIGHT, CROSS, TEE_LEFT, &widths));
        out.push('\n');

        let last = self.rows.len().saturating_sub(1);
        for (index, row) in self.rows.iter().enumerate() {
            match row {
                Row::Cells(cells) => {
                    out.push_str(&self.line(cells, &widths));
                    out.push('\n');
                }
                Row::Separator => {
                    // A trailing separator would double the bottom border.
                    if index == last {
                        continue;
                    }
                    out.push_str(&self.border(TEE_RIGHT, CROSS, TEE_LEFT, &widths));
                    out.push('\n');
                }
            }
        }

        out.push_str(&self.border(BOTTOM_LEFT, TEE_UP, BOTTOM_RIGHT, &widths));
        out
    }

    fn border(&self, left: char, joint: char, right: char, widths: &[usize]) -> String {
        let mut out = String::new();
        out.push(left);
        for (index, column) in widths.iter().enumerate() {
            if index > 0 {
                out.push(joint);
            }
            out.extend(std::iter::repeat_n(HORIZONTAL, column + 2));
        }
        out.push(right);
        out
    }

    /// Builds a content line, drawing `│` between columns.
    fn line(&self, cells: &[String], widths: &[usize]) -> String {
        let mut out = String::new();
        out.push(VERTICAL);
        for (index, column) in widths.iter().enumerate() {
            let empty = String::new();
            let cell = cells.get(index).unwrap_or(&empty);
            out.push(' ');
            out.push_str(cell);
            out.extend(std::iter::repeat_n(' ', column.saturating_sub(width(cell))));
            out.push(' ');
            out.push(VERTICAL);
        }
        out
    }
}

/// Width in characters, so multi-byte text does not break the alignment.
fn width(text: &str) -> usize {
    text.chars().count()
}

/// Formats kibibytes with a binary unit. The unit is the largest one whose value
/// rounds to at least one whole unit *at that unit's own precision*, so a quantity
/// that would print as `1024.0 MiB` is promoted to `1.0 GiB` instead.
pub fn format_kib(kib: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = KIB * 1024;
    const GIB: u64 = MIB * 1024;
    const TIB: u64 = GIB * 1024;

    // (unit in bytes, suffix, decimal places used when printing it)
    const UNITS: [(u64, &str, usize); 4] = [
        (TIB, "TiB", 1),
        (GIB, "GiB", 1),
        (MIB, "MiB", 1),
        (KIB, "KiB", 0),
    ];

    let bytes = kib.saturating_mul(KIB);

    for (unit, suffix, decimals) in UNITS {
        let value = bytes as f64 / unit as f64;
        let scale = 10_f64.powi(decimals as i32);
        if (value * scale).round() >= scale {
            return format!("{value:.decimals$} {suffix}");
        }
    }

    format!("{bytes} B")
}

/// Formats uptime as `1d 2h 3m 4s`, omitting zero components. A fully zero
/// uptime renders as `0s` rather than an empty string.
pub fn format_uptime(seconds: u64) -> String {
    let days = seconds / 86_400;
    let hours = seconds % 86_400 / 3_600;
    let minutes = seconds % 3_600 / 60;
    let secs = seconds % 60;

    let parts: Vec<String> = [(days, 'd'), (hours, 'h'), (minutes, 'm'), (secs, 's')]
        .into_iter()
        .filter(|(value, _)| *value > 0)
        .map(|(value, unit)| format!("{value}{unit}"))
        .collect();

    if parts.is_empty() {
        return "0s".to_string();
    }
    parts.join(" ")
}

fn format_load(info: &HostInfo) -> String {
    format!(
        "{:.2} / {:.2} / {:.2}",
        info.load.one, info.load.five, info.load.fifteen
    )
}

/// Percentage of `part` over `total`, or `None` when `total` is zero.
fn percentage(part: u64, total: u64) -> Option<u64> {
    if total == 0 {
        return None;
    }
    Some((part.saturating_mul(100) / total).min(100))
}

fn with_percentage(value: impl std::fmt::Display, part: u64, total: u64) -> String {
    match percentage(part, total) {
        Some(percent) => format!("{value} ({percent}%)"),
        None => value.to_string(),
    }
}

/// Renders a snapshot as the CLI table: system identity first, then resources.
pub fn render_host_info(info: &HostInfo) -> String {
    let mut table = Table::new(vec!["FIELD".to_string(), "VALUE".to_string()]);

    let os_version = info.os.version.trim();
    let os = match (info.os.name.trim(), os_version.is_empty()) {
        (name, true) if !name.is_empty() => name.to_string(),
        (name, _) if !name.is_empty() => format!("{name} {os_version}"),
        _ => "unknown".to_string(),
    };

    table
        .push(vec!["Hostname".into(), info.hostname.clone()])
        .push(vec!["Operating system".into(), os])
        .push(vec!["Kernel".into(), info.os.kernel.clone()])
        .push(vec!["Architecture".into(), info.os.arch.clone()])
        .separator()
        .push(vec!["CPU".into(), info.cpu_model.clone()])
        .push(vec!["Logical cores".into(), info.logical_cores.to_string()])
        .push(vec!["Uptime".into(), format_uptime(info.uptime_seconds)])
        .push(vec!["Load average".into(), format_load(info)]);

    let memory = &info.memory;
    let used = memory.total_kib.saturating_sub(memory.available_kib);
    table.push(vec!["Memory total".into(), format_kib(memory.total_kib)]);
    if memory.total_kib > 0 {
        table.push(vec![
            "Memory used".into(),
            with_percentage(format_kib(used), used, memory.total_kib),
        ]);
        table.push(vec![
            "Memory available".into(),
            with_percentage(
                format_kib(memory.available_kib),
                memory.available_kib,
                memory.total_kib,
            ),
        ]);
    }

    if memory.swap_total_kib > 0 {
        let swap_used = memory.swap_total_kib.saturating_sub(memory.swap_free_kib);
        table.push(vec!["Swap total".into(), format_kib(memory.swap_total_kib)]);
        table.push(vec![
            "Swap used".into(),
            with_percentage(format_kib(swap_used), swap_used, memory.swap_total_kib),
        ]);
    } else {
        table.push(vec!["Swap".into(), "not configured".into()]);
    }

    table.render()
}
