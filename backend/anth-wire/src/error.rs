//! One error type for the whole protocol.
//!
//! v0.1.0 defined a separate `*ErrorKind` enum per feature — six of them —
//! each mapped by hand to an HTTP status and again to a CLI exit code, so the
//! two mappings could disagree. Here a failure names a [`ErrorKind`], and both
//! the status and the exit code are derived from it in exactly one place.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Why a request failed, in the terms a caller can act on.
///
/// The kinds are the union of what the v0.1.0 per-feature enums expressed, so
/// porting a feature does not need a new enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// The named thing does not exist.
    NotFound,
    /// The caller may not do this: a policy switch is off, or the target is
    /// protected.
    Forbidden,
    /// The caller has not proved who they are, or the proof expired.
    ///
    /// Distinct from [`ErrorKind::Forbidden`] on purpose: `401` says "send a
    /// credential" and `403` says "this credential is not enough". A client that
    /// answered both with the same page would offer a login form after a login,
    /// and the operator would have no way to tell a wrong password from a
    /// missing permission.
    Unauthorized,
    /// The request was well formed but conflicts with current state.
    Conflict,
    /// The request itself is malformed: a bad path, an unknown field, a value
    /// outside the accepted set.
    Invalid,
    /// The host lacks what this needs: a binary, a kernel feature, a capability.
    Unsupported,
    /// The payload exceeds a documented limit.
    TooLarge,
    /// The daemon failed for a reason it does not classify. The message is for
    /// a human reading logs, not for branching on.
    Internal,
}

impl ErrorKind {
    /// HTTP status for this kind.
    ///
    /// This is the single mapping: `api` calls it rather than matching on the
    /// kind again, which is how v0.1.0's route table drifted from its CLI codes.
    pub fn http_status(self) -> u16 {
        match self {
            ErrorKind::NotFound => 404,
            ErrorKind::Forbidden => 403,
            ErrorKind::Unauthorized => 401,
            ErrorKind::Conflict => 409,
            ErrorKind::Invalid => 400,
            ErrorKind::Unsupported => 501,
            ErrorKind::TooLarge => 413,
            ErrorKind::Internal => 500,
        }
    }

    /// Process exit code for this kind.
    ///
    /// `1` for everything except a request the caller got wrong (`2`), matching
    /// the convention v0.1.0 documented in `docs/cli.md`.
    pub fn exit_code(self) -> u8 {
        match self {
            ErrorKind::Invalid => 2,
            _ => 1,
        }
    }

    /// The kind a status came from, for a caller that has one and needs the
    /// other.
    ///
    /// The inverse of [`ErrorKind::http_status`], kept beside it so the two
    /// cannot disagree. A status outside the table is `Internal`, because the
    /// alternative — inventing a kind — would let a route answer `418` and have
    /// the CLI report it as a bad request.
    pub fn from_http_status(status: u16) -> Self {
        match status {
            404 => ErrorKind::NotFound,
            403 => ErrorKind::Forbidden,
            401 => ErrorKind::Unauthorized,
            409 => ErrorKind::Conflict,
            400 => ErrorKind::Invalid,
            501 => ErrorKind::Unsupported,
            413 => ErrorKind::TooLarge,
            _ => ErrorKind::Internal,
        }
    }

    /// Stable machine-readable tag, used in the JSON error body.
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorKind::NotFound => "not_found",
            ErrorKind::Forbidden => "forbidden",
            ErrorKind::Unauthorized => "unauthorized",
            ErrorKind::Conflict => "conflict",
            ErrorKind::Invalid => "invalid",
            ErrorKind::Unsupported => "unsupported",
            ErrorKind::TooLarge => "too_large",
            ErrorKind::Internal => "internal",
        }
    }
}

/// A failure crossing the wire.
///
/// `kind` is what a client branches on; `message` is for a human.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireError {
    pub kind: ErrorKind,
    pub message: String,
}

impl WireError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::NotFound, message)
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Forbidden, message)
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Invalid, message)
    }

    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Unsupported, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Internal, message)
    }
}

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind.as_str(), self.message)
    }
}

impl std::error::Error for WireError {}
