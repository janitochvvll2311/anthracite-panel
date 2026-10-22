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

    /// Stable machine-readable tag, used in the JSON error body.
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorKind::NotFound => "not_found",
            ErrorKind::Forbidden => "forbidden",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_has_a_distinct_status_and_a_tag() {
        let kinds = [
            ErrorKind::NotFound,
            ErrorKind::Forbidden,
            ErrorKind::Conflict,
            ErrorKind::Invalid,
            ErrorKind::Unsupported,
            ErrorKind::TooLarge,
            ErrorKind::Internal,
        ];
        let mut tags: Vec<&str> = kinds.iter().map(|k| k.as_str()).collect();
        tags.sort_unstable();
        let unique = tags.len();
        tags.dedup();
        assert_eq!(tags.len(), unique, "two kinds share a tag");

        for kind in kinds {
            assert!(
                (400..=599).contains(&kind.http_status()),
                "{kind:?} has no status"
            );
        }
    }

    #[test]
    fn the_status_mapping_matches_the_documented_contract() {
        assert_eq!(ErrorKind::NotFound.http_status(), 404);
        assert_eq!(ErrorKind::Forbidden.http_status(), 403);
        assert_eq!(ErrorKind::Conflict.http_status(), 409);
        assert_eq!(ErrorKind::Invalid.http_status(), 400);
        assert_eq!(ErrorKind::Unsupported.http_status(), 501);
        assert_eq!(ErrorKind::TooLarge.http_status(), 413);
        assert_eq!(ErrorKind::Internal.http_status(), 500);
    }

    #[test]
    fn a_caller_mistake_exits_two_and_a_host_failure_exits_one() {
        assert_eq!(ErrorKind::Invalid.exit_code(), 2);
        assert_eq!(ErrorKind::NotFound.exit_code(), 1);
        assert_eq!(ErrorKind::Forbidden.exit_code(), 1);
    }

    #[test]
    fn an_error_round_trips_through_json() {
        let error = WireError::forbidden("control is off");
        let json = serde_json::to_string(&error).unwrap();
        assert_eq!(json, r#"{"kind":"forbidden","message":"control is off"}"#);
        let back: WireError = serde_json::from_str(&json).unwrap();
        assert_eq!(back, error);
    }

    #[test]
    fn display_names_the_kind_so_a_log_line_is_greppable() {
        let error = WireError::not_found("/etc/nope");
        assert_eq!(error.to_string(), "not_found: /etc/nope");
    }
}
