//! Process table values, pagination and the signal allowlist.
//!
//! `pid` alone is not an identity: the kernel recycles it. Every mutation
//! carries the `start_ticks` the caller saw, so a signal cannot land on the
//! process that inherited the number.

use serde::{Deserialize, Serialize};

use crate::Page;

/// Largest number of arguments one spawn request carries.
pub const MAX_SPAWN_ARGS: usize = 64;

/// Largest single argument, in bytes.
pub const MAX_SPAWN_ARG_BYTES: usize = 4096;

/// One row of the process table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub pid: u32,
    pub ppid: u32,
    pub uid: u32,
    /// Account name for `uid`, or the number when `/etc/passwd` has no entry.
    pub user: String,
    /// `/proc/<pid>/comm`.
    pub name: String,
    /// `/proc/<pid>/cmdline` joined with spaces, or `[name]` for kernel threads.
    pub command: String,
    /// One-letter `/proc` state: `R`, `S`, `D`, `Z`, `T`, ...
    pub state: String,
    /// `utime + stime` in clock ticks.
    pub cpu_ticks: u64,
    /// Resident set size from `/proc/<pid>/status`.
    pub rss_kib: u64,
    pub threads: u32,
    /// Start time in clock ticks since boot; with `pid`, the stable identity.
    pub start_ticks: u64,
}

/// One page of the process table plus the facts needed to read it.
///
/// CPU percentages are a client calculation: the daemon never guesses an
/// interval. Two snapshots and `clock_ticks` are enough.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessSnapshot {
    #[serde(flatten)]
    pub page: Page<ProcessInfo>,
    /// Kernel clock ticks per second.
    pub clock_ticks: u64,
    /// When the snapshot was taken, in milliseconds since the Unix epoch.
    pub timestamp_ms: u64,
    /// Whether `[process] control` allows signals.
    pub control_enabled: bool,
    /// Whether `[process] spawn` allows starting programs.
    pub spawn_enabled: bool,
    /// Programs a spawn request may start, as configured.
    #[serde(default)]
    pub spawn_allow: Vec<String>,
}

/// The signals an operator may send.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProcessSignal {
    Term,
    Kill,
    Hup,
    Int,
    Quit,
    Stop,
    Cont,
}

impl ProcessSignal {
    /// Every allowed signal, in the order the UI lists them.
    pub const ALL: [Self; 7] = [
        Self::Term,
        Self::Kill,
        Self::Hup,
        Self::Int,
        Self::Quit,
        Self::Stop,
        Self::Cont,
    ];

    /// Stable lowercase name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Term => "term",
            Self::Kill => "kill",
            Self::Hup => "hup",
            Self::Int => "int",
            Self::Quit => "quit",
            Self::Stop => "stop",
            Self::Cont => "cont",
        }
    }

    /// Parses a name, case-insensitively. `sigterm` is accepted as an alias.
    pub fn parse(text: &str) -> Option<Self> {
        let name = text.trim().to_ascii_lowercase();
        let name = name.strip_prefix("sig").unwrap_or(&name);
        Self::ALL.into_iter().find(|signal| signal.as_str() == name)
    }
}

/// What a successful spawn reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessStarted {
    pub pid: u32,
    /// Program as requested; the allowlist matched its canonical target.
    pub program: String,
    pub args: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_names_round_trip_and_accept_the_sig_prefix() {
        for signal in ProcessSignal::ALL {
            assert_eq!(ProcessSignal::parse(signal.as_str()), Some(signal));
            assert_eq!(
                ProcessSignal::parse(&format!("SIG{}", signal.as_str().to_uppercase())),
                Some(signal)
            );
        }
        assert_eq!(ProcessSignal::parse("nope"), None);
    }

    #[test]
    fn a_snapshot_serializes_flat() {
        let snapshot = ProcessSnapshot {
            page: Page::new(Vec::new()),
            clock_ticks: 100,
            timestamp_ms: 7,
            control_enabled: false,
            spawn_enabled: false,
            spawn_allow: Vec::new(),
        };
        let json = serde_json::to_value(&snapshot).expect("serialize");
        assert_eq!(json["clock_ticks"], 100);
        assert_eq!(json["timestamp_ms"], 7);
        assert!(json["items"].is_array());
        let back: ProcessSnapshot = serde_json::from_value(json).expect("deserialize");
        assert_eq!(back, snapshot);
    }
}
