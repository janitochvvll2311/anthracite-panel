//! Bodies for mutations that report a value instead of a snapshot.
//!
//! Every mutating route answers with one of these, so a custom client does not
//! have to parse a bare JSON object by hand.

use serde::{Deserialize, Serialize};

/// A mutation whose only output is a ready-to-print message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    pub message: String,
}

/// A file or directory was deleted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileDeleted {
    pub path: String,
}

/// A file or directory moved to the trash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileTrashed {
    pub path: String,
    pub id: String,
}

/// A trash entry was restored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRestored {
    pub id: String,
    pub path: String,
}

/// The trash was emptied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrashEmptied {
    pub removed: usize,
}

/// An archive was created.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveCreated {
    pub path: String,
}

/// An archive was extracted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveExtracted {
    pub path: String,
    pub entries: Vec<String>,
}

/// A signal was delivered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessSignalled {
    pub pid: u32,
    pub signal: String,
}

/// The daemon accepted a restart and is about to replace itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Restarting {
    /// The pid the new image keeps.
    pub pid: u32,
}

/// The daemon accepted a reboot and scheduled it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rebooting {
    /// Seconds until the host goes down.
    pub delay_s: u32,
}
