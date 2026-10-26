//! One error vocabulary for the whole protocol.
//!
//! A failure names an [`ErrorKind`]; HTTP status and CLI exit code are derived
//! from it in exactly one place. The `code` is a stable machine tag for clients
//! that need to branch without parsing a human message.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Why a request failed, in terms a caller can act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    NotFound,
    Forbidden,
    Unauthorized,
    Conflict,
    Invalid,
    Unsupported,
    TooLarge,
    /// The caller is sending too many requests; retry after a delay.
    TooMany,
    Internal,
}

impl ErrorKind {
    /// HTTP status for this kind.
    pub fn http_status(self) -> u16 {
        match self {
            ErrorKind::NotFound => 404,
            ErrorKind::Forbidden => 403,
            ErrorKind::Unauthorized => 401,
            ErrorKind::Conflict => 409,
            ErrorKind::Invalid => 400,
            ErrorKind::Unsupported => 501,
            ErrorKind::TooLarge => 413,
            ErrorKind::TooMany => 429,
            ErrorKind::Internal => 500,
        }
    }

    /// Process exit code for this kind. A bad request is `2`; everything else
    /// is `1`, matching the CLI convention.
    pub fn exit_code(self) -> u8 {
        match self {
            ErrorKind::Invalid => 2,
            _ => 1,
        }
    }

    /// Stable snake_case name, matching the wire serialization.
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorKind::NotFound => "not_found",
            ErrorKind::Forbidden => "forbidden",
            ErrorKind::Unauthorized => "unauthorized",
            ErrorKind::Conflict => "conflict",
            ErrorKind::Invalid => "invalid",
            ErrorKind::Unsupported => "unsupported",
            ErrorKind::TooLarge => "too_large",
            ErrorKind::TooMany => "too_many",
            ErrorKind::Internal => "internal",
        }
    }

    /// Stable machine tag used when a caller does not provide a narrower code.
    pub fn default_code(self) -> &'static str {
        match self {
            ErrorKind::NotFound => "not_found",
            ErrorKind::Forbidden => "forbidden",
            ErrorKind::Unauthorized => "unauthorized",
            ErrorKind::Conflict => "conflict",
            ErrorKind::Invalid => "invalid",
            ErrorKind::Unsupported => "unsupported",
            ErrorKind::TooLarge => "too_large",
            ErrorKind::TooMany => "too_many",
            ErrorKind::Internal => "internal",
        }
    }
}

/// A failure crossing the wire.
///
/// `kind` is what a client branches on; `code` is the stable machine tag;
/// `message` is for a human and is never a branch condition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireError {
    pub kind: ErrorKind,
    pub code: String,
    pub message: String,
}

impl WireError {
    pub fn new(kind: ErrorKind, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind,
            code: code.into(),
            message: message.into(),
        }
    }

    /// Builds an error with the kind's default code.
    pub fn of(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self::new(kind, kind.default_code(), message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::of(ErrorKind::NotFound, message)
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::of(ErrorKind::Forbidden, message)
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::of(ErrorKind::Unauthorized, message)
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::of(ErrorKind::Invalid, message)
    }

    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::of(ErrorKind::Unsupported, message)
    }

    pub fn too_large(message: impl Into<String>) -> Self {
        Self::of(ErrorKind::TooLarge, message)
    }

    pub fn too_many(message: impl Into<String>) -> Self {
        Self::of(ErrorKind::TooMany, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::of(ErrorKind::Internal, message)
    }
}

impl fmt::Display for WireError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for WireError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_and_exit_code_are_derived_from_one_kind() {
        assert_eq!(ErrorKind::TooMany.http_status(), 429);
        assert_eq!(ErrorKind::TooMany.exit_code(), 1);
        assert_eq!(ErrorKind::Invalid.http_status(), 400);
        assert_eq!(ErrorKind::Invalid.exit_code(), 2);
        assert_eq!(ErrorKind::Internal.http_status(), 500);
        assert_eq!(ErrorKind::Internal.exit_code(), 1);
    }

    #[test]
    fn default_codes_are_stable_and_distinct() {
        let kinds = [
            ErrorKind::NotFound,
            ErrorKind::Forbidden,
            ErrorKind::Unauthorized,
            ErrorKind::Conflict,
            ErrorKind::Invalid,
            ErrorKind::Unsupported,
            ErrorKind::TooLarge,
            ErrorKind::TooMany,
            ErrorKind::Internal,
        ];
        let mut codes: Vec<_> = kinds.iter().map(|kind| kind.default_code()).collect();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), kinds.len());
    }

    #[test]
    fn error_round_trips_as_json() {
        let error = WireError::new(ErrorKind::TooMany, "rate_limited", "slow down");
        let text = serde_json::to_string(&error).expect("serialize");
        assert_eq!(
            text,
            r#"{"kind":"too_many","code":"rate_limited","message":"slow down"}"#
        );
        let parsed: WireError = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(parsed, error);
    }
}
