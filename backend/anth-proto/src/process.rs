//! Process inspection and signal requests.
//!
//! The daemon reads Linux `/proc` directly. These types are the transport
//! contract only: no parsing, no policy and no OS calls live here.

use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// Signals the panel is willing to deliver. The allowlist is deliberately
/// small; it is not a general-purpose `kill` interface.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
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
    /// Stable lowercase name used by the CLI and the API.
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
}

impl FromStr for ProcessSignal {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "term" => Ok(Self::Term),
            "kill" => Ok(Self::Kill),
            "hup" => Ok(Self::Hup),
            "int" => Ok(Self::Int),
            "quit" => Ok(Self::Quit),
            "stop" => Ok(Self::Stop),
            "cont" => Ok(Self::Cont),
            _ => Err(format!(
                "unknown signal `{value}`; expected one of term, kill, hup, int, quit, stop, cont"
            )),
        }
    }
}

impl std::fmt::Display for ProcessSignal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// One process as seen in a snapshot.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct ProcessInfo {
    pub pid: u32,
    pub ppid: u32,
    pub uid: u32,
    pub user: String,
    pub name: String,
    pub command: String,
    pub state: String,
    /// `utime + stime`, in clock ticks. Clients turn deltas into CPU percent.
    pub cpu_ticks: u64,
    pub rss_kib: u64,
    pub threads: u32,
    /// Process start time in clock ticks since boot. Together with the pid it
    /// identifies a process across snapshots even if the pid is reused.
    pub start_ticks: u64,
}

/// One point-in-time view of the process table.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct ProcessSnapshot {
    pub processes: Vec<ProcessInfo>,
    /// `sysconf(_SC_CLK_TCK)`, used to turn tick deltas into CPU percent.
    pub clock_ticks: u64,
    /// Wall-clock time of the snapshot in milliseconds since the Unix epoch.
    pub timestamp_ms: u64,
    /// Whether the daemon allows signalling processes.
    pub control_enabled: bool,
    /// Number of processes found before any truncation.
    pub total: usize,
    /// Whether `processes` was cut at the daemon's row cap.
    pub truncated: bool,
    /// Human-readable notice when rows or command lines were shortened.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}
