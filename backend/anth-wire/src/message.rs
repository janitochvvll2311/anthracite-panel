//! Messages that travel through a frame.
//!
//! One `Request` and one `Response` enum cover the protocol. Phase 0 defines the
//! handshake and the host snapshot; later features add variants here and one arm
//! in `anth-panel::dispatch`.

use serde::{Deserialize, Serialize};

use crate::error::WireError;

/// Bumped when a change would make an older peer misread a message.
///
/// Pre-1.0.0 breaking changes are allowed; see
/// `docs/design/protocol-changelog.md`.
pub const PROTOCOL_VERSION: u16 = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Request {
    /// The mandatory first frame on a socket connection.
    Hello {
        protocol_version: u16,
        /// `<kind>/<version>`, e.g. `cli/0.3.0`.
        client: String,
        /// Reserved for post-1.0 feature negotiation; empty today.
        #[serde(default)]
        features: Vec<String>,
    },
    /// One snapshot of the host the daemon runs on.
    HostInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Response {
    /// Answer to [`Request::Hello`].
    Welcome {
        protocol_version: u16,
        server: String,
        version: String,
        /// Features this daemon understands. Empty today; unknown features are
        /// ignored by a client.
        #[serde(default)]
        features: Vec<String>,
    },
    HostInfo(HostInfo),
    Error(WireError),
}

impl Response {
    pub fn error(&self) -> Option<&WireError> {
        match self {
            Response::Error(error) => Some(error),
            _ => None,
        }
    }

    pub fn into_result(self) -> Result<Self, WireError> {
        match self {
            Response::Error(error) => Err(error),
            other => Ok(other),
        }
    }
}

/// Operating system identification from `os-release` and the kernel.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct OsInfo {
    pub name: String,
    pub version: String,
    pub kernel: String,
    pub arch: String,
}

/// The three load averages `/proc/loadavg` reports.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct LoadAverage {
    pub one: f64,
    pub five: f64,
    pub fifteen: f64,
}

/// Memory and swap totals in kibibytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MemoryInfo {
    pub total_kib: u64,
    pub available_kib: u64,
    pub swap_total_kib: u64,
    pub swap_free_kib: u64,
}

impl MemoryInfo {
    pub fn used_kib(&self) -> u64 {
        self.total_kib.saturating_sub(self.available_kib)
    }

    pub fn swap_used_kib(&self) -> u64 {
        self.swap_total_kib.saturating_sub(self.swap_free_kib)
    }
}

/// A point-in-time description of the host.
///
/// Unreadable fields degrade to `unknown` or zero; a missing `/proc` entry is
/// not a reason to fail the request.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct HostInfo {
    pub hostname: String,
    pub os: OsInfo,
    pub uptime_seconds: u64,
    pub cpu_model: String,
    pub logical_cores: u32,
    pub load: LoadAverage,
    pub memory: MemoryInfo,
    /// Unix epoch milliseconds when the snapshot was collected.
    pub collected_at_ms: u64,
}
