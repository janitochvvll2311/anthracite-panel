//! The process table: one `/proc` scan, the signal guard and the spawn guard.
//!
//! Reading is pure and cheap enough to run on the blocking pool. Mutations are
//! gated by two independent switches: `[process] control` for signals and
//! `[process] spawn` plus `spawn_allow` for starting programs.
//!
//! The page cursor is the offset into the sorted snapshot. It is opaque to
//! clients; a new snapshot can reorder rows, so a caller that keeps a cursor
//! across ticks sees whatever that offset holds now.

mod parse;
mod signal;
mod spawn;

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use anth_wire::{
    Page, PageRequest, ProcessInfo, ProcessSignal, ProcessSnapshot, ProcessStarted, WireError,
};

use parse::{parse_cmdline, parse_comm, parse_stat, parse_status, read_users};

/// The process service, built once from the policy the daemon read at startup.
#[derive(Debug, Clone)]
pub struct ProcessService {
    control: bool,
    spawn: bool,
    spawn_allow: Vec<String>,
}

impl ProcessService {
    pub fn new(control: bool, spawn: bool, spawn_allow: Vec<String>) -> Self {
        Self {
            control,
            spawn,
            spawn_allow,
        }
    }

    /// One page of the process table, busiest first.
    pub fn snapshot(&self, request: &PageRequest) -> Result<ProcessSnapshot, WireError> {
        if !request.valid() {
            return Err(WireError::invalid(
                "the page limit or cursor is out of bounds",
            ));
        }
        let offset = match request.cursor.as_deref() {
            None => 0,
            Some(cursor) => cursor
                .parse::<usize>()
                .map_err(|_| WireError::invalid("the process page cursor is not valid"))?,
        };

        let users = fs::read_to_string("/etc/passwd")
            .map(|text| read_users(&text))
            .unwrap_or_default();

        let mut entries = Vec::new();
        let directory = fs::read_dir("/proc")
            .map_err(|error| WireError::internal(format!("reading /proc: {error}")))?;
        for entry in directory.flatten() {
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<u32>().ok())
            else {
                continue;
            };
            let path = entry.path();
            let Some(stat) = read_stat(&path) else {
                // A process that vanished between `read_dir` and `stat` is not
                // a reason to fail the whole snapshot, and neither is one this
                // uid cannot read.
                continue;
            };
            entries.push(row(pid, &path, &users, stat));
        }

        entries.sort_by(|left, right| {
            right
                .cpu_ticks
                .cmp(&left.cpu_ticks)
                .then(left.pid.cmp(&right.pid))
        });

        let total = entries.len();
        let limit = usize::from(request.limit());
        let items: Vec<ProcessInfo> = entries.into_iter().skip(offset).take(limit).collect();
        let served = offset.saturating_add(items.len());
        let next = (served < total).then(|| served.to_string());

        Ok(ProcessSnapshot {
            page: Page {
                items,
                next,
                total: Some(total as u64),
            },
            clock_ticks: clock_ticks(),
            timestamp_ms: now_ms(),
            control_enabled: self.control,
            spawn_enabled: self.spawn,
            spawn_allow: self.spawn_allow.clone(),
        })
    }

    /// Delivers one allowlisted signal. See [`signal::send`].
    pub fn signal(
        &self,
        pid: u32,
        start_ticks: u64,
        signal: ProcessSignal,
        group: bool,
    ) -> Result<(), WireError> {
        signal::send(self.control, pid, start_ticks, signal, group)
    }

    /// Starts one allowlisted program. See [`spawn::spawn`].
    pub fn spawn(
        &self,
        program: &str,
        args: &[String],
        cwd: Option<&str>,
    ) -> Result<ProcessStarted, WireError> {
        spawn::spawn(self.spawn, &self.spawn_allow, program, args, cwd)
    }
}

fn row(
    pid: u32,
    path: &Path,
    users: &std::collections::HashMap<u32, String>,
    stat: parse::StatFields,
) -> ProcessInfo {
    let status = fs::read_to_string(path.join("status"))
        .map(|text| parse_status(&text))
        .unwrap_or_default();
    let uid = status.uid.unwrap_or_else(|| {
        fs::metadata(path)
            .map(|metadata| metadata.uid())
            .unwrap_or(0)
    });
    let name = fs::read_to_string(path.join("comm"))
        .map(|text| parse_comm(&text))
        .unwrap_or_default();
    let command = fs::read(path.join("cmdline"))
        .ok()
        .and_then(|bytes| parse_cmdline(&bytes))
        .unwrap_or_else(|| format!("[{name}]"));
    ProcessInfo {
        pid,
        ppid: stat.ppid,
        uid,
        user: users.get(&uid).cloned().unwrap_or_else(|| uid.to_string()),
        name,
        command,
        state: stat.state,
        cpu_ticks: stat.cpu_ticks,
        rss_kib: status.rss_kib,
        threads: status.threads.unwrap_or(stat.threads),
        start_ticks: stat.start_ticks,
    }
}

fn read_stat(path: &Path) -> Option<parse::StatFields> {
    let text = fs::read_to_string(path.join("stat")).ok()?;
    parse_stat(&text)
}

/// Kernel clock ticks per second; the fallback is the Linux default.
fn clock_ticks() -> u64 {
    static TICKS: OnceLock<u64> = OnceLock::new();
    *TICKS.get_or_init(|| {
        let value = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
        if value > 0 { value as u64 } else { 100 }
    })
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}
