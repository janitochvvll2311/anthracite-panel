//! Rendering of host information as a bordered text table.

use anth_proto::{
    AccountSnapshot, ArchiveListing, FileKind, FileListing, FirewallSnapshot, HostInfo,
    NetworkSnapshot, ProcessSnapshot, Response, TrashListing, VolumeDevice, VolumeSnapshot,
};

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

/// Renders a process snapshot as a table. CPU is accumulated CPU time, not a
/// live percentage; the web component computes percentages from tick deltas.
pub fn render_process_snapshot(snapshot: &ProcessSnapshot) -> String {
    let mut table = Table::new(vec![
        "PID".to_string(),
        "PPID".to_string(),
        "USER".to_string(),
        "CPU".to_string(),
        "MEM".to_string(),
        "STATE".to_string(),
        "COMMAND".to_string(),
    ]);

    for process in &snapshot.processes {
        let cpu = if snapshot.clock_ticks == 0 {
            "?".to_string()
        } else {
            format!(
                "{:.1}s",
                process.cpu_ticks as f64 / snapshot.clock_ticks as f64
            )
        };

        table.push(vec![
            process.pid.to_string(),
            process.ppid.to_string(),
            process.user.clone(),
            cpu,
            format_kib(process.rss_kib),
            process.state.clone(),
            elide(&process.command, 80),
        ]);
    }

    let mut output = table.render();
    if let Some(warning) = &snapshot.warning {
        output.push('\n');
        output.push_str("Warning: ");
        output.push_str(warning);
    }
    output
}

/// Renders a network snapshot as a table. Traffic counters are cumulative;
/// clients that need a live rate compute it from consecutive snapshots.
pub fn render_network_snapshot(snapshot: &NetworkSnapshot) -> String {
    let mut table = Table::new(vec![
        "IFACE".to_string(),
        "STATE".to_string(),
        "ADDRESS".to_string(),
        "SPEED".to_string(),
        "RX".to_string(),
        "TX".to_string(),
    ]);

    for interface in &snapshot.interfaces {
        let addresses = if interface.addresses.is_empty() {
            interface.mac.clone()
        } else {
            interface
                .addresses
                .iter()
                .map(|address| format!("{}/{}", address.address, address.prefix_len))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let speed = interface
            .speed_mbps
            .map(|speed| format!("{speed} Mb/s"))
            .unwrap_or_else(|| "-".to_string());

        table.push(vec![
            interface.name.clone(),
            interface.state.to_uppercase(),
            elide(&addresses, 48),
            speed,
            format_bytes(interface.rx_bytes),
            format_bytes(interface.tx_bytes),
        ]);
    }

    let mut output = table.render();
    if let Some(warning) = &snapshot.warning {
        output.push('\n');
        output.push_str("Warning: ");
        output.push_str(warning);
    }
    output
}

/// Renders the storage inventory. Block devices come first as a flat list with
/// tree indentation, then the mounts as a second table.
pub fn render_volume_snapshot(snapshot: &VolumeSnapshot) -> String {
    let mut devices = Table::new(vec![
        "DEVICE".to_string(),
        "SIZE".to_string(),
        "TYPE".to_string(),
        "FSTYPE".to_string(),
        "LABEL".to_string(),
        "MOUNT".to_string(),
    ]);

    for device in &snapshot.devices {
        let depth = device_depth(&snapshot.devices, device);
        let indent = "  ".repeat(depth);
        let mount = if device.mountpoints.is_empty() {
            "-".to_string()
        } else {
            device.mountpoints.join(", ")
        };
        devices.push(vec![
            format!("{indent}{}", device.name),
            format_bytes(device.size_bytes),
            device.kind.to_string(),
            device.fstype.clone().unwrap_or_else(|| "-".to_string()),
            device.label.clone().unwrap_or_else(|| "-".to_string()),
            elide(&mount, 40),
        ]);
    }

    let mut output = if snapshot.devices.is_empty() {
        "No block devices found.".to_string()
    } else {
        devices.render()
    };

    if snapshot.mounts.is_empty() {
        output.push_str("\n\nNo mounts found.");
    } else {
        output.push_str("\n\n");
        let mut mounts = Table::new(vec![
            "MOUNTPOINT".to_string(),
            "SOURCE".to_string(),
            "FSTYPE".to_string(),
            "USED".to_string(),
            "AVAILABLE".to_string(),
            "MODE".to_string(),
        ]);
        for mount in &snapshot.mounts {
            let (used, available) = match mount.usage {
                Some(usage) => (
                    format!(
                        "{} ({:.0}%)",
                        format_bytes(usage.used_bytes),
                        percent(usage.used_bytes, usage.total_bytes)
                    ),
                    format_bytes(usage.available_bytes),
                ),
                None => ("-".to_string(), "-".to_string()),
            };
            let mode = match (mount.read_only, mount.virtual_fs) {
                (true, true) => "ro, virtual",
                (true, false) => "ro",
                (false, true) => "virtual",
                (false, false) => "rw",
            };
            mounts.push(vec![
                mount.mountpoint.clone(),
                elide(&mount.source, 40),
                mount.fstype.clone(),
                used,
                available,
                mode.to_string(),
            ]);
        }
        output.push_str(&mounts.render());
    }

    if let Some(warning) = &snapshot.warning {
        output.push('\n');
        output.push_str("Warning: ");
        output.push_str(warning);
    }
    output
}

/// Two tables: users, then groups. `NSS` marks an entry the panel cannot write,
/// and `?` marks a lock state it could not read; both are facts, not gaps.
pub fn render_account_snapshot(snapshot: &AccountSnapshot) -> String {
    let mut users = Table::new(vec![
        "USER".to_string(),
        "UID".to_string(),
        "GID".to_string(),
        "GROUP".to_string(),
        "STATE".to_string(),
        "SHELL".to_string(),
        "SOURCE".to_string(),
    ]);

    for user in &snapshot.users {
        let state = match user.locked {
            Some(true) => "locked",
            Some(false) => "unlocked",
            None => "?",
        };
        users.push(vec![
            user.name.clone(),
            user.uid.to_string(),
            user.gid.to_string(),
            elide(&user.group, 20),
            state.to_string(),
            elide(&user.shell, 24),
            user.source.to_string(),
        ]);
    }

    let mut output = if snapshot.users.is_empty() {
        "No users found.".to_string()
    } else {
        users.render()
    };

    if snapshot.groups.is_empty() {
        output.push_str("\n\nNo groups found.");
    } else {
        output.push_str("\n\n");
        let mut groups = Table::new(vec![
            "GROUP".to_string(),
            "GID".to_string(),
            "MEMBERS".to_string(),
            "SOURCE".to_string(),
        ]);
        for group in &snapshot.groups {
            groups.push(vec![
                group.name.clone(),
                group.gid.to_string(),
                elide(&group.members.join(", "), 48),
                group.source.to_string(),
            ]);
        }
        output.push_str(&groups.render());
    }

    if snapshot.truncated {
        output.push_str(&format!(
            "\n\nShowing {} of {} users and {} of {} groups.",
            snapshot.users.len(),
            snapshot.total_users,
            snapshot.groups.len(),
            snapshot.total_groups
        ));
    }

    if let Some(warning) = &snapshot.warning {
        output.push('\n');
        output.push_str("Warning: ");
        output.push_str(warning);
    }
    output
}

/// Nesting depth from the parent links, so the CLI draws the same tree the
/// protocol carries flat. Cycles are impossible in `lsblk` output, but the walk
/// is still bounded by the list length.
fn device_depth(devices: &[VolumeDevice], device: &VolumeDevice) -> usize {
    let mut depth = 0;
    let mut current = device;
    while let Some(parent) = &current.parent {
        depth += 1;
        if depth >= devices.len() {
            break;
        }
        match devices.iter().find(|candidate| &candidate.name == parent) {
            Some(found) => current = found,
            None => break,
        }
    }
    depth
}

fn percent(part: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        part as f64 * 100.0 / total as f64
    }
}

/// One archive listing: members, sizes and the compression ratio when the
/// format reports both sizes.
pub fn render_archive_listing(listing: &ArchiveListing) -> String {
    let mut rows = Vec::with_capacity(listing.entries.len());
    for entry in &listing.entries {
        let (kind, name) = match entry.kind {
            // Zip directory entries already end in `/`; adding a second one
            // turns `docs/` into `docs//`.
            FileKind::Directory => (
                "dir",
                if entry.name.ends_with('/') {
                    entry.name.clone()
                } else {
                    format!("{}/", entry.name)
                },
            ),
            _ => ("file", entry.name.clone()),
        };
        let size = if entry.kind == FileKind::Directory {
            "-".to_string()
        } else {
            format_bytes(entry.size)
        };
        let ratio = match entry.compressed {
            Some(compressed) if entry.size > 0 => {
                format!("{}%", compressed.saturating_mul(100) / entry.size)
            }
            _ => "-".to_string(),
        };
        rows.push(vec![
            kind.to_string(),
            name,
            size,
            entry
                .compressed
                .map(format_bytes)
                .unwrap_or_else(|| "-".to_string()),
            ratio,
        ]);
    }

    let mut table = Table::new(vec![
        "TYPE".to_string(),
        "NAME".to_string(),
        "SIZE".to_string(),
        "STORED".to_string(),
        "RATIO".to_string(),
    ]);
    for row in rows {
        table.push(row);
    }
    let mut out = table.render();

    let format = listing
        .format
        .map(|format| format.label().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    out.push_str(&format!(
        "\n{} holds {} entries ({format}).\n",
        listing.path, listing.total
    ));
    if let Some(warning) = &listing.warning {
        out.push_str(&format!("{warning}\n"));
    }
    out
}

/// Formats one daemon response as the text the CLI prints. Errors carry the
/// same `[ERROR]` prefix as successful commands carry `[OK]`.
pub fn render_response(response: &Response) -> String {
    match response {
        Response::Ok { message } => format!("[OK] {message}"),
        Response::Error { detail } => format!("[ERROR] {detail}"),
        Response::HostInfo(info) => render_host_info(info),
        Response::ProcessSnapshot(snapshot) => render_process_snapshot(snapshot),
        Response::ProcessSignalled { pid, signal } => {
            format!("[OK] sent {signal} to process {pid}")
        }
        Response::ProcessStarted(started) => {
            let args = if started.args.is_empty() {
                String::new()
            } else {
                format!(" {}", started.args.join(" "))
            };
            format!(
                "[OK] started {}{args} as pid {}",
                started.program, started.pid
            )
        }
        Response::ProcessSpawnError(error) => format!("[ERROR] {}", error.detail),
        Response::FileArchiveCreated { path } => format!("[OK] created {path}"),
        Response::ArchiveListing(listing) => render_archive_listing(listing),
        Response::FileArchiveExtracted { path, entries } => {
            let noun = if *entries == 1 { "entry" } else { "entries" };
            format!("[OK] extracted {entries} {noun} into {path}")
        }
        Response::NetworkSnapshot(snapshot) => render_network_snapshot(snapshot),
        Response::NetworkStateChanged { name, up } => {
            let state = if *up { "up" } else { "down" };
            format!("[OK] brought {name} {state}")
        }
        Response::NetworkChanged { message } => format!("[OK] {message}"),
        Response::NetworkError(error) => format!("[ERROR] {}", error.detail),
        Response::AccountSnapshot(snapshot) => render_account_snapshot(snapshot),
        Response::AccountChanged { message } => format!("[OK] {message}"),
        Response::AccountError(error) => format!("[ERROR] {}", error.detail),
        Response::VolumeSnapshot(snapshot) => render_volume_snapshot(snapshot),
        Response::VolumeChanged { message } => format!("[OK] {message}"),
        Response::VolumeError(error) => format!("[ERROR] {}", error.detail),
        Response::FirewallSnapshot(snapshot) => render_firewall_snapshot(snapshot),
        Response::FirewallChanged { message } => format!("[OK] {message}"),
        Response::FirewallError(error) => format!("[ERROR] {}", error.detail),
        Response::FileListing(listing) => render_file_listing(listing),
        Response::FileChunk(chunk) => format!(
            "[ERROR] unexpected file chunk for {} ({} bytes)",
            chunk.path,
            chunk.bytes.len()
        ),
        Response::FileWritten(written) => {
            format!("[OK] wrote {} ({} bytes)", written.path, written.size)
        }
        Response::FileCreated { path } => format!("[OK] created {path}"),
        Response::FileDeleted { path } => format!("[OK] deleted {path}"),
        Response::FileRenamed { from, to } => format!("[OK] renamed {from} to {to}"),
        Response::FileWriteStarted { id } => format!("[OK] upload {id} started"),
        Response::FileWriteAccepted { id, received } => {
            format!("[OK] upload {id} received {received} bytes")
        }
        Response::FileWriteAborted { id } => format!("[OK] upload {id} aborted"),
        Response::FileTrashed { path, id } => format!("[OK] moved {path} to the trash as {id}"),
        Response::TrashListing(listing) => render_trash_listing(listing),
        Response::FileRestored { id, path } => format!("[OK] restored {id} to {path}"),
        Response::FileTrashEmptied { removed } => {
            format!("[OK] emptied the trash ({removed} entries removed)")
        }
        Response::FileError(error) => format!("[ERROR] {}", error.detail),
        // A CLI subcommand never opens a terminal, so these are only reachable
        // if a daemon volunteers frames nobody asked for.
        Response::TerminalOpened(opened) => {
            format!(
                "[ERROR] unexpected terminal session {} from the daemon",
                opened.id
            )
        }
        Response::TerminalOutput { id, bytes } => {
            format!(
                "[ERROR] unexpected terminal output for session {id} ({} bytes)",
                bytes.len()
            )
        }
        Response::TerminalExited { id, status } => {
            format!("[ERROR] unexpected terminal exit for session {id} (status {status})")
        }
    }
}

/// Renders the managed firewall rules. A missing table is a normal state, not
/// an error: the daemon has simply never created it.
pub fn render_firewall_snapshot(snapshot: &FirewallSnapshot) -> String {
    if !snapshot.initialized {
        return "Firewall table is not initialized.".to_string();
    }

    let mut table = Table::new(vec![
        "HANDLE".to_string(),
        "ACTION".to_string(),
        "PROTO".to_string(),
        "PORT".to_string(),
        "COMMENT".to_string(),
    ]);

    for rule in &snapshot.rules {
        table.push(vec![
            rule.handle.to_string(),
            rule.action.to_uppercase(),
            rule.protocol
                .map(|protocol| protocol.to_string())
                .unwrap_or_else(|| "-".to_string()),
            rule.port
                .map(|port| port.to_string())
                .unwrap_or_else(|| "-".to_string()),
            rule.comment.clone().unwrap_or_else(|| "-".to_string()),
        ]);
    }

    let mut output = table.render();
    if let Some(warning) = &snapshot.warning {
        output.push('\n');
        output.push_str("Warning: ");
        output.push_str(warning);
    }
    output
}

/// Renders one directory listing as a table, followed by a warning when the
/// daemon shortened it.
pub fn render_file_listing(listing: &FileListing) -> String {
    let mut table = Table::new(vec![
        "TYPE".to_string(),
        "NAME".to_string(),
        "SIZE".to_string(),
        "MODIFIED".to_string(),
    ]);

    for entry in &listing.entries {
        let kind = match entry.kind {
            FileKind::Directory => "dir",
            FileKind::File => "file",
            FileKind::Symlink => "link",
            FileKind::Other => "other",
        };
        let modified = entry
            .modified_ms
            .map(format_timestamp)
            .unwrap_or_else(|| "?".to_string());
        table.push(vec![
            kind.to_string(),
            entry.name.clone(),
            if matches!(entry.kind, FileKind::Directory) {
                "-".to_string()
            } else {
                format_bytes(entry.size)
            },
            modified,
        ]);
    }

    let mut output = table.render();
    if let Some(warning) = &listing.warning {
        output.push('\n');
        output.push_str("Warning: ");
        output.push_str(warning);
    }
    output
}

/// The trash table. Kept beside the listing renderer because both answer a
/// "what is here" question and share the size and timestamp helpers.
pub fn render_trash_listing(listing: &TrashListing) -> String {
    let mut table = Table::new(vec![
        "TYPE".to_string(),
        "NAME".to_string(),
        "SIZE".to_string(),
        "DELETED FROM".to_string(),
    ]);

    for entry in &listing.entries {
        let kind = match entry.kind {
            FileKind::Directory => "dir",
            FileKind::File => "file",
            FileKind::Symlink => "link",
            FileKind::Other => "other",
        };
        table.push(vec![
            kind.to_string(),
            entry.name.clone(),
            if matches!(entry.kind, FileKind::Directory) {
                "-".to_string()
            } else {
                format_bytes(entry.size)
            },
            entry.original_path.clone(),
        ]);
    }

    let mut output = table.render();
    let retention = match listing.max_bytes {
        Some(max) => format!(" of {}", format_bytes(max)),
        None => String::new(),
    };
    output.push_str(&format!(
        "\nTrash holds {} entries using {}{retention}.",
        listing.total,
        format_bytes(listing.used_bytes)
    ));
    if !listing.enabled {
        output.push_str("\nThe trash is disabled ([files] trash), so entries can be listed and restored but not created.");
    }
    if let Some(warning) = &listing.warning {
        output.push_str("\nWarning: ");
        output.push_str(warning);
    }
    output
}

fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;

    let value = bytes as f64;
    if value >= GIB {
        format!("{:.1} GiB", value / GIB)
    } else if value >= MIB {
        format!("{:.1} MiB", value / MIB)
    } else if value >= KIB {
        format!("{:.1} KiB", value / KIB)
    } else {
        format!("{bytes} B")
    }
}

/// Minimal UTC formatter so the CLI does not pull in a date dependency.
fn format_timestamp(millis: u64) -> String {
    let seconds = millis / 1000;
    let days = (seconds / 86_400) as i64;
    let remaining = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = remaining / 3_600;
    let minute = (remaining % 3_600) / 60;
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02} UTC")
}

/// Converts days since 1970-01-01 to a Gregorian date (Howard Hinnant's
/// algorithm), avoiding a time crate for one label.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = if month <= 2 { y + 1 } else { y };
    (year, month as u32, day as u32)
}

fn elide(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }

    let mut shortened: String = text.chars().take(limit.saturating_sub(1)).collect();
    shortened.push('…');
    shortened
}

/// The installed plugins as a table, with any loading problem below it.
///
/// A plugin with no capabilities still appears: it is installed, and the admin
/// needs to see that it can currently do nothing. A problem is printed rather
/// than dropped, because a plugin that silently fails to load looks like a bug
/// in the panel.
pub fn render_plugin_listing(
    plugins: &[crate::plugins::InstalledPlugin],
    problems: &[String],
) -> String {
    let mut table = Table::new(vec![
        "ID".to_string(),
        "VERSION".to_string(),
        "STATE".to_string(),
        "CAPABILITIES".to_string(),
        "UI".to_string(),
    ]);

    for plugin in plugins {
        table.push(vec![
            plugin.id.clone(),
            plugin.version.clone(),
            if plugin.enabled {
                "enabled"
            } else {
                "disabled"
            }
            .to_string(),
            if plugin.capabilities.is_empty() {
                "-".to_string()
            } else {
                plugin.capabilities.join(", ")
            },
            if plugin.ui.is_empty() {
                "-".to_string()
            } else {
                plugin
                    .ui
                    .iter()
                    .map(|anchor| anchor.slot.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            },
        ]);
    }

    let mut out = table.render();
    if plugins.is_empty() {
        out.push_str("No plugins are installed.\n");
    }
    for problem in problems {
        out.push_str(&format!("[WARN] {problem}\n"));
    }
    out
}
