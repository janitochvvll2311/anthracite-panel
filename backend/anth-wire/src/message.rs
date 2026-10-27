//! Messages that travel through a frame.
//!
//! One `Request` and one `Response` enum cover the protocol. Phase 0 defines the
//! handshake and the host snapshot; later features add variants here and one arm
//! in `anth-panel::dispatch`.

use serde::{Deserialize, Serialize};

use crate::error::WireError;
use crate::network::NetworkSnapshot;
use crate::page::Page;
use crate::process::{ProcessSignal, ProcessSnapshot, ProcessStarted};
use crate::terminal::{TerminalOpened, TerminalOutput};

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
    /// One snapshot of the host's network interfaces and default routes.
    Network,
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
    /// Moves one entry to the freedesktop.org trash. Requires `files.write`
    /// and `files.trash`.
    FilesTrash { path: String },
    /// Lists every trash directory the daemon can see.
    FilesTrashList,
    /// Restores one trashed entry to its recorded path.
    FilesTrashRestore {
        id: String,
        #[serde(default)]
        overwrite: bool,
    },
    /// Deletes everything the trash holds.
    FilesTrashEmpty,
    /// Compresses one entry. `destination` and `format` default from `path`.
    FilesArchive {
        path: String,
        #[serde(default)]
        destination: Option<String>,
        #[serde(default)]
        format: Option<ArchiveFormat>,
    },
    /// Lists one archive without extracting it.
    FilesArchiveList { path: String },
    /// Extracts one archive. `into` defaults beside the archive.
    FilesArchiveExtract {
        path: String,
        #[serde(default)]
        into: Option<String>,
    },
    /// Opens one PTY and returns its opaque handle.
    TerminalOpen { cols: u16, rows: u16 },
    /// Waits at most `wait_ms` for output from one session.
    TerminalRead { id: String, wait_ms: u32 },
    /// Sends keystrokes to one session.
    TerminalWrite { id: String, bytes_base64: String },
    /// Updates the window size and signals the process group.
    TerminalResize { id: String, cols: u16, rows: u16 },
    /// Ends one session. Closing an already-closed handle is not an error.
    TerminalClose { id: String },
    /// One page of the process table, sorted by accumulated CPU.
    Processes {
        #[serde(default)]
        limit: Option<u16>,
        #[serde(default)]
        cursor: Option<String>,
    },
    /// Delivers an allowlisted signal to one process. Requires
    /// `process.control`.
    ProcessSignal {
        pid: u32,
        /// The value the caller saw; a mismatch means the pid was recycled.
        start_ticks: u64,
        signal: ProcessSignal,
        /// Signal the process group led by `pid` instead of only the process.
        #[serde(default)]
        group: bool,
    },
    /// Starts a program from `process.spawn_allow`. Requires `process.spawn`.
    ProcessSpawn {
        /// Absolute path of the program.
        program: String,
        #[serde(default)]
        args: Vec<String>,
        /// Absolute working directory, or the daemon's own.
        #[serde(default)]
        cwd: Option<String>,
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
    /// One network inventory snapshot.
    Network(NetworkSnapshot),
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
    /// The entry moved to the trash.
    FilesTrashed {
        path: String,
        id: String,
    },
    /// The contents of every trash directory.
    FilesTrashList(TrashListing),
    /// The trash entry was restored.
    FilesRestored {
        id: String,
        path: String,
    },
    /// The trash was emptied.
    FilesTrashEmptied {
        removed: usize,
    },
    /// The archive was created.
    FilesArchiveCreated {
        path: String,
    },
    /// The members of one archive.
    FilesArchiveList(ArchiveListing),
    /// The archive was extracted.
    FilesArchiveExtracted {
        path: String,
        entries: Vec<String>,
    },
    /// A terminal now exists under its opaque handle.
    TerminalOpened(TerminalOpened),
    /// One read from a terminal, or the end of the program.
    TerminalOutput(TerminalOutput),
    /// One page of the process table with the policy the caller must respect.
    Processes(ProcessSnapshot),
    /// The signal was delivered.
    ProcessSignalled {
        pid: u32,
        signal: ProcessSignal,
    },
    /// The program was started; it is not supervised.
    ProcessStarted(ProcessStarted),
    /// A write, resize or close succeeded and has no value to return.
    Accepted,
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

/// The kind of one entry inside an archive or the trash.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileKind {
    #[serde(rename = "file")]
    File,
    #[serde(rename = "dir")]
    Directory,
    #[serde(rename = "symlink")]
    Symlink,
    #[serde(rename = "other")]
    Other,
}

/// Archive formats the daemon can read or write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArchiveFormat {
    #[serde(rename = "zip")]
    Zip,
    #[serde(rename = "tar")]
    Tar,
    #[serde(rename = "tar.gz")]
    TarGz,
    #[serde(rename = "tar.xz")]
    TarXz,
    #[serde(rename = "7z")]
    SevenZ,
}

impl ArchiveFormat {
    /// The name clients and the CLI print.
    pub fn label(self) -> &'static str {
        match self {
            Self::Zip => "zip",
            Self::Tar => "tar",
            Self::TarGz => "tar.gz",
            Self::TarXz => "tar.xz",
            Self::SevenZ => "7z",
        }
    }
}

/// One member of an archive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveEntry {
    pub name: String,
    pub kind: FileKind,
    /// Uncompressed size in bytes.
    pub size: u64,
    /// Size on disk inside the archive, when the format reports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compressed: Option<u64>,
}

/// One archive and the members it holds.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ArchiveListing {
    pub path: String,
    pub format: Option<ArchiveFormat>,
    pub entries: Vec<ArchiveEntry>,
    pub total: usize,
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

/// One entry held by the freedesktop trash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrashEntry {
    /// Identifier under the trash `files/` and `info/` directories.
    pub id: String,
    /// Name the entry had before it was deleted.
    pub name: String,
    /// Path the entry is restored to.
    pub original_path: String,
    /// Raw `DeletionDate` from the `.trashinfo` file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<String>,
    pub kind: FileKind,
    pub size: u64,
    /// Host path of the trash directory holding the entry.
    pub location: String,
    /// Modification time of the stored entry, used to order the listing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stored_ms: Option<u64>,
}

/// The contents of every trash directory the daemon can see.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TrashListing {
    pub entries: Vec<TrashEntry>,
    pub total: usize,
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    /// Whether `[files] trash` is on, so the panel knows if trashing works.
    pub enabled: bool,
    /// Bytes currently held, and the retention ceiling when one is set.
    pub used_bytes: u64,
    pub max_bytes: Option<u64>,
}
