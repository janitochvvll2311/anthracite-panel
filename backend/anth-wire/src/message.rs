//! Messages that travel through a frame.
//!
//! One `Request` and one `Response` enum cover the protocol. Phase 0 defines the
//! handshake and the host snapshot; later features add variants here and one arm
//! in `anth-panel::dispatch`.

use serde::{Deserialize, Serialize};

use crate::error::WireError;
use crate::page::Page;

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
    /// Replaces this process with a fresh copy of the same command line, which
    /// re-reads the configuration.
    Restart,
    /// Asks the host to reboot after `delay_s` seconds.
    SystemReboot {
        /// Seconds to wait before the host goes down. At most
        /// `anth_panel::lifecycle::MAX_REBOOT_DELAY_S`.
        #[serde(default)]
        delay_s: u32,
    },
    /// One page of a directory listing.
    FilesList {
        /// Path relative to the configured file root; empty or `/` is the root.
        #[serde(default)]
        path: String,
        #[serde(default)]
        limit: Option<u16>,
        #[serde(default)]
        cursor: Option<String>,
    },
    /// A chunk of a UTF-8 text file.
    FilesRead {
        path: String,
        #[serde(default)]
        offset: u64,
        #[serde(default)]
        max_bytes: Option<u64>,
    },
    /// Deletes one regular file under the configured root. Requires
    /// `files.write`; directories are refused.
    FilesDelete { path: String },
    /// Writes one UTF-8 text file under the configured root. Requires
    /// `files.write`.
    FilesWrite {
        path: String,
        content: String,
        /// Fail with `conflict` when the file already exists instead of
        /// replacing it.
        #[serde(default)]
        create: bool,
    },
    /// Uploads arbitrary bytes under the configured root. Requires
    /// `files.write`.
    FilesUpload {
        path: String,
        /// The file content, base64 encoded.
        content_base64: String,
        /// Fail with `conflict` when the file already exists instead of
        /// replacing it.
        #[serde(default)]
        create: bool,
    },
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
    /// The restart was accepted; this process is about to be replaced.
    Restarting {
        /// The pid the new image keeps.
        pid: u32,
    },
    /// The reboot was accepted and is scheduled.
    Rebooting {
        /// Seconds until the host goes down.
        delay_s: u32,
    },
    /// One page of a directory listing.
    FilesList(Page<FileEntry>),
    /// A chunk of a text file.
    FilesRead(FileChunk),
    /// The file was deleted.
    FilesDeleted {
        path: String,
    },
    /// The file was written.
    FilesWritten(FileWritten),
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

/// One entry in a directory listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    /// `file`, `dir`, `symlink` or `other`.
    pub kind: String,
    pub size: u64,
    /// Unix epoch milliseconds, or 0 when the platform cannot report it.
    pub modified_ms: u64,
}

/// A chunk of a UTF-8 text file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChunk {
    /// The path relative to the file root, using `/` separators.
    pub path: String,
    pub offset: u64,
    /// Full size of the file in bytes.
    pub size: u64,
    /// Whether this chunk reaches the end of the file.
    pub eof: bool,
    pub text: String,
}

/// The result of writing a UTF-8 text file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileWritten {
    /// The path relative to the file root, using `/` separators.
    pub path: String,
    /// Bytes written.
    pub size: u64,
}
