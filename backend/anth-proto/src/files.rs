//! File explorer requests and responses.
//!
//! These types are the transport contract only. The daemon owns path
//! resolution, filesystem access and the write policy.

use serde::{Deserialize, Serialize};
use std::fmt;

use crate::b64;

/// Preferred chunk size for file reads and writes.
///
/// Base64 expands bytes by roughly one third, so a 256 KiB chunk still fits in
/// the protocol's 1 MiB line limit with room for the JSON envelope.
pub const DEFAULT_FILE_CHUNK: u32 = 256 * 1024;

/// Hard cap the daemon accepts for a single read or write chunk.
pub const MAX_FILE_CHUNK: u32 = 512 * 1024;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FileKind {
    File,
    Directory,
    Symlink,
    Other,
}

/// One directory entry.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub name: String,
    /// Virtual absolute path. `/` is the configured filesystem root.
    pub path: String,
    pub kind: FileKind,
    pub size: u64,
    pub modified_ms: Option<u64>,
    pub hidden: bool,
    /// Raw link target when `kind` is `Symlink`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

/// One directory listing.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct FileListing {
    pub path: String,
    pub parent: Option<String>,
    pub entries: Vec<FileEntry>,
    /// Number of entries found before the row cap was applied.
    pub total: usize,
    pub truncated: bool,
    /// Human-readable notice when the listing was shortened.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    /// Whether the daemon accepts the write operations.
    pub write_enabled: bool,
}

/// One chunk of a file.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct FileChunk {
    pub path: String,
    pub offset: u64,
    /// Total file size in bytes.
    pub size: u64,
    pub eof: bool,
    #[serde(with = "b64")]
    pub bytes: Vec<u8>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct FileWritten {
    pub path: String,
    pub size: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FileErrorKind {
    NotFound,
    PermissionDenied,
    NotADirectory,
    IsADirectory,
    InvalidPath,
    AlreadyExists,
    DirectoryNotEmpty,
    TooLarge,
    Disabled,
    Io,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct FileError {
    pub kind: FileErrorKind,
    pub detail: String,
}

impl FileError {
    pub fn new(kind: FileErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }
}

impl fmt::Display for FileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for FileError {}
