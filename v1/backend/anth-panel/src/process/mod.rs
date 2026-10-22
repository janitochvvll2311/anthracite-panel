//! Linux process inspection, signals and spawns.
//!
//! `parse` is pure: it turns `/proc` contents into numbers and strings.
//! `fit` shortens a snapshot to the protocol line cap. `signal` and `spawn` are
//! the two ways this module changes the host, and both enforce the policy here.

use self::prelude::*;

/// Maximum rows returned in one snapshot. The protocol is newline-delimited and
/// one line is capped at 1 MiB, so the serialized snapshot is also fitted to
/// that budget below.
pub const MAX_PROCESSES: usize = 2000;
/// Maximum command-line length in a snapshot, in characters.
pub const MAX_COMMAND_CHARS: usize = 256;
/// Commands are shortened down to this many characters before rows are dropped
/// to satisfy the protocol line limit.
pub const MIN_COMMAND_CHARS: usize = 8;
pub(crate) const CLOCK_TICKS_FALLBACK: u64 = 100;

/// The daemon's process policy, resolved from `[process]` once per request.
///
/// One struct instead of three arguments so a new switch cannot be forgotten at
/// one of the call sites, and so the snapshot and the gate read the same values.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProcessPolicy {
    /// Whether signals are accepted.
    pub control: bool,
    /// Whether spawning is accepted.
    pub spawn: bool,
    /// Absolute programs a spawn may start. Empty denies everything.
    pub spawn_allow: Vec<String>,
}

impl ProcessPolicy {
    pub fn new(config: &crate::config::ProcessConfig) -> Self {
        Self {
            control: config.control,
            spawn: config.spawn,
            spawn_allow: config.spawn_allow.clone(),
        }
    }
}

/// Reads the live process table.
pub fn collect(policy: &ProcessPolicy) -> ProcessSnapshot {
    collect_from("/", policy)
}

/// Reads a process table from an alternative filesystem root.
pub fn collect_from(root: impl AsRef<Path>, policy: &ProcessPolicy) -> ProcessSnapshot {
    let root = root.as_ref();
    let users = read_users(&root.join("etc/passwd"));
    let (mut processes, command_truncated) = read_processes(&root.join("proc"), &users);
    let total = processes.len();

    // Truncation keeps the busiest processes, not the lowest pids.
    processes.sort_by(|left, right| {
        right
            .cpu_ticks
            .cmp(&left.cpu_ticks)
            .then_with(|| left.pid.cmp(&right.pid))
    });

    let truncated = total > MAX_PROCESSES;
    if truncated {
        processes.truncate(MAX_PROCESSES);
    }

    let snapshot = ProcessSnapshot {
        processes,
        clock_ticks: clock_ticks(),
        timestamp_ms: timestamp_ms(),
        control_enabled: policy.control,
        spawn_enabled: policy.spawn,
        // An allowlist the client cannot see is a form the operator fills in
        // blind, so the snapshot carries it. Entries are filtered the same way
        // `spawn` filters them, so the UI cannot offer what the gate rejects.
        spawn_allow: policy
            .spawn_allow
            .iter()
            .filter(|entry| Path::new(entry).is_absolute())
            .cloned()
            .collect(),
        total,
        truncated,
        warning: None,
    };

    fit_snapshot(snapshot, command_truncated)
}

mod fit;
mod parse;
mod prelude;

use self::fit::{clock_ticks, fit_snapshot, timestamp_ms};
use self::parse::{read_processes, read_users};

mod signal;
mod spawn;

pub use signal::{SignalError, send_signal};
pub use spawn::{MAX_SPAWN_ARG_BYTES, MAX_SPAWN_ARGS, spawn};

#[cfg(test)]
mod tests;
