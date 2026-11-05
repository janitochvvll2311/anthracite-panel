//! The one error model: a code, a message and optional structured detail.
//!
//! HTTP, the CLI and both clients read the same envelope, and the mapping from a
//! code to a status and an exit code is defined here once.

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Every error the API can report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    /// An app refused, or a capability was not granted.
    Denied,
    /// No session, or an expired one.
    Unauthenticated,
    /// The arguments were refused.
    Invalid,
    /// The resource does not exist.
    NotFound,
    /// The payload is over the limit.
    TooLarge,
    /// A provider is missing or degraded.
    Unavailable,
    /// The id exists, the export is duplicated, or dependents would break.
    Conflict,
    /// `If-Match` does not match.
    PreconditionFailed,
    /// The verb is not declared.
    MethodNotAllowed,
    /// An app bug or a trap; the message is sanitized.
    Internal,
    /// The call exceeded its fuel, memory or wall budget.
    Exhausted,
}

impl ErrorKind {
    /// The wire code, stable and part of the contract.
    pub const fn code(self) -> &'static str {
        match self {
            Self::Denied => "denied",
            Self::Unauthenticated => "unauthenticated",
            Self::Invalid => "invalid",
            Self::NotFound => "not-found",
            Self::TooLarge => "too-large",
            Self::Unavailable => "unavailable",
            Self::Conflict => "conflict",
            Self::PreconditionFailed => "precondition-failed",
            Self::MethodNotAllowed => "method-not-allowed",
            Self::Internal => "internal",
            Self::Exhausted => "exhausted",
        }
    }

    /// The HTTP status this code answers with.
    pub const fn status(self) -> u16 {
        match self {
            Self::Denied => 403,
            Self::Unauthenticated => 401,
            Self::Invalid => 400,
            Self::NotFound => 404,
            Self::TooLarge => 413,
            Self::Unavailable | Self::Exhausted => 503,
            Self::Conflict => 409,
            Self::PreconditionFailed => 412,
            Self::MethodNotAllowed => 405,
            Self::Internal => 500,
        }
    }

    /// The kind a wire code names. An unknown code is not an error: clients
    /// treat it as `internal`, which is what forward compatibility requires.
    pub fn from_code(code: &str) -> Option<Self> {
        Some(match code {
            "denied" => Self::Denied,
            "unauthenticated" => Self::Unauthenticated,
            "invalid" => Self::Invalid,
            "not-found" => Self::NotFound,
            "too-large" => Self::TooLarge,
            "unavailable" => Self::Unavailable,
            "conflict" => Self::Conflict,
            "precondition-failed" => Self::PreconditionFailed,
            "method-not-allowed" => Self::MethodNotAllowed,
            "internal" => Self::Internal,
            "exhausted" => Self::Exhausted,
            _ => return None,
        })
    }

    /// The CLI exit code, so a script can branch without parsing text.
    pub const fn exit_code(self) -> u8 {
        match self {
            Self::Internal => 1,
            Self::Invalid | Self::Conflict | Self::PreconditionFailed | Self::MethodNotAllowed => 2,
            Self::Denied | Self::Unauthenticated => 3,
            Self::NotFound => 4,
            Self::Unavailable | Self::Exhausted => 5,
            Self::TooLarge => 6,
        }
    }
}

/// One error, with everything the envelope needs.
#[derive(Debug, Clone, PartialEq)]
pub struct Error {
    kind: ErrorKind,
    message: String,
    app: Option<String>,
    detail: Option<Value>,
}

impl Error {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            app: None,
            detail: None,
        }
    }

    pub fn denied(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Denied, message)
    }

    pub fn unauthenticated(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Unauthenticated, message)
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Invalid, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::NotFound, message)
    }

    pub fn too_large(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::TooLarge, message)
    }

    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Unavailable, message)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Conflict, message)
    }

    pub fn precondition_failed(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::PreconditionFailed, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Internal, message)
    }

    pub fn exhausted(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Exhausted, message)
    }

    pub fn with_app(mut self, app: impl Into<String>) -> Self {
        self.app = Some(app.into());
        self
    }

    pub fn with_detail(mut self, detail: Value) -> Self {
        self.detail = Some(detail);
        self
    }

    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn status(&self) -> u16 {
        self.kind.status()
    }

    pub fn exit_code(&self) -> u8 {
        self.kind.exit_code()
    }

    /// The JSON body: `{"error":{"code","message","app?","detail?"}}`.
    pub fn envelope(&self) -> ErrorEnvelope {
        ErrorEnvelope {
            error: ErrorBody {
                code: self.kind.code().to_string(),
                message: self.message.clone(),
                app: self.app.clone(),
                detail: self.detail.clone(),
            },
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind.code(), self.message)
    }
}

impl std::error::Error for Error {}

/// The serialized envelope. Serializes to `{"error":{…}}` and reads back, so a
/// client parses errors and responses with the same types.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorEnvelope {
    pub error: ErrorBody,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_statuses_and_exits_agree() {
        let cases = [
            (ErrorKind::Denied, "denied", 403, 3),
            (ErrorKind::Unauthenticated, "unauthenticated", 401, 3),
            (ErrorKind::Invalid, "invalid", 400, 2),
            (ErrorKind::NotFound, "not-found", 404, 4),
            (ErrorKind::TooLarge, "too-large", 413, 6),
            (ErrorKind::Unavailable, "unavailable", 503, 5),
            (ErrorKind::Conflict, "conflict", 409, 2),
            (ErrorKind::PreconditionFailed, "precondition-failed", 412, 2),
            (ErrorKind::MethodNotAllowed, "method-not-allowed", 405, 2),
            (ErrorKind::Internal, "internal", 500, 1),
            (ErrorKind::Exhausted, "exhausted", 503, 5),
        ];
        for (kind, code, status, exit) in cases {
            assert_eq!(kind.code(), code);
            assert_eq!(kind.status(), status);
            assert_eq!(kind.exit_code(), exit);
        }
    }

    #[test]
    fn the_envelope_omits_empty_fields() {
        let value = serde_json::to_value(Error::invalid("bad").envelope()).expect("json");
        assert_eq!(value["error"]["code"], "invalid");
        assert_eq!(value["error"]["message"], "bad");
        assert!(value["error"].get("app").is_none());
        assert!(value["error"].get("detail").is_none());
    }

    #[test]
    fn an_envelope_reads_back() {
        let error = Error::too_large("over the cap")
            .with_app("org.example.notes")
            .with_detail(serde_json::json!({"limit": 32}));
        let text = serde_json::to_string(&error.envelope()).expect("json");
        let back: ErrorEnvelope = serde_json::from_str(&text).expect("parse");
        assert_eq!(back.error.code, "too-large");
        assert_eq!(back.error.message, "over the cap");
        assert_eq!(back.error.app.as_deref(), Some("org.example.notes"));
        assert_eq!(back.error.detail.expect("detail")["limit"], 32);
    }

    #[test]
    fn an_unknown_code_is_not_a_known_kind() {
        assert_eq!(ErrorKind::from_code("invalid"), Some(ErrorKind::Invalid));
        assert_eq!(ErrorKind::from_code("something-new"), None);
    }

    #[test]
    fn the_envelope_carries_app_and_detail() {
        let error = Error::conflict("required by another app")
            .with_app("org.anth.fs")
            .with_detail(serde_json::json!({"dependents": ["org.example.notes"]}));
        let value = serde_json::to_value(error.envelope()).expect("json");
        assert_eq!(value["error"]["app"], "org.anth.fs");
        assert_eq!(
            value["error"]["detail"]["dependents"][0],
            "org.example.notes"
        );
    }
}
