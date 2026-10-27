//! The error vocabulary the ported trash and archive modules share.
//!
//! Those modules came from the v0.1 tree, where every file failure carried a
//! richer kind than [`WireError`]. Keeping the internal kind here lets the
//! proven guard code stay unchanged; the service boundary maps it once.

use std::fmt;

use anth_wire::{ErrorKind, WireError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    Conflict,
    Io,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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

impl From<FileError> for WireError {
    fn from(error: FileError) -> Self {
        match error.kind {
            FileErrorKind::NotFound => WireError::not_found(error.detail),
            FileErrorKind::PermissionDenied => WireError::forbidden(error.detail),
            FileErrorKind::InvalidPath => {
                WireError::new(ErrorKind::Invalid, "invalid_path", error.detail)
            }
            FileErrorKind::NotADirectory
            | FileErrorKind::IsADirectory
            | FileErrorKind::DirectoryNotEmpty => {
                WireError::new(ErrorKind::Invalid, "invalid", error.detail)
            }
            FileErrorKind::AlreadyExists => {
                WireError::new(ErrorKind::Conflict, "exists", error.detail)
            }
            FileErrorKind::TooLarge => WireError::too_large(error.detail),
            FileErrorKind::Disabled => {
                WireError::new(ErrorKind::Forbidden, "disabled", error.detail)
            }
            FileErrorKind::Conflict => {
                WireError::new(ErrorKind::Conflict, "conflict", error.detail)
            }
            FileErrorKind::Io => WireError::internal(error.detail),
        }
    }
}
