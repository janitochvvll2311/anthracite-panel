//! The request and reply envelopes the host speaks, and nothing else.
//!
//! The host hands a guest one JSON envelope per invocation and reads one JSON
//! reply back. Both shapes are a contract with the daemon, so they live here
//! rather than in each plugin: a guest that invents its own field names is a
//! guest the host answers with a `200` and the wrong body.
//!
//! Decoding is tolerant where the host is loose and strict where the host is
//! strict. The daemon sends `body` as a *string* (an anchor body is not required
//! to be JSON) and always sends `query`, so [`http::HttpRequest`] models both.
//! It never sends a malformed envelope, but a guest reached through `plugin call`
//! can be handed anything, which is why decoding returns a [`Reply::bad_request`]
//! instead of panicking.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use serde::Deserialize;
use serde_json::{Value, json};

/// One invocation, as the daemon sends it.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Request {
    /// An HTTP anchor, or one anchor reached through `plugin call`.
    Http(http::HttpRequest),
    /// `plugin call <id> '{"kind":"cli","argv":[...]}'`.
    Cli(CliRequest),
}

/// The `{"kind":"cli","argv":[...]}` envelope.
///
/// `argv` is free-form text the caller wrote, never an argv the host will run:
/// a plugin may only execute through a capability, so these words are a request
/// to the plugin, not a program.
#[derive(Debug, Deserialize)]
pub struct CliRequest {
    #[serde(default)]
    pub argv: Vec<String>,
}

/// How a guest answers.
///
/// A reply is always an envelope, even for a plain text answer, because the host
/// reads `status` and `body` and would otherwise have to guess a status for a
/// bare string. Guessing is the host answering for the plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    status: u16,
    body: Value,
}

impl Reply {
    /// An envelope with an explicit status.
    pub fn new(status: u16, body: Value) -> Self {
        Self { status, body }
    }

    /// `200` with a JSON body.
    pub fn ok(body: Value) -> Self {
        Self::new(200, body)
    }

    /// `200` with a JSON object carrying one `message`, the shape the panel's
    /// plugin widgets read.
    pub fn message(text: impl Into<String>) -> Self {
        Self::ok(json!({ "message": text.into() }))
    }

    /// `404`, `409`, `400`, `403` or `500`: every status a guest needs.
    ///
    /// Naming the status is the guest's job. A capability that was refused by the
    /// host, a route the plugin does not implement and a caller sending a bad body
    /// are three different answers, and a plugin that returns `200` for all three
    /// is a plugin the operator cannot debug.
    pub fn error(status: u16, detail: impl Into<String>) -> Self {
        Self::new(status, json!({ "error": detail.into() }))
    }

    /// `400`: the caller's envelope or body was not what the plugin expects.
    pub fn bad_request(detail: impl Into<String>) -> Self {
        Self::error(400, detail)
    }

    /// `404`: no route for that method and path.
    pub fn not_found(detail: impl Into<String>) -> Self {
        Self::error(404, detail)
    }

    /// `500`: the plugin could not do what was asked.
    pub fn failed(detail: impl Into<String>) -> Self {
        Self::error(500, detail)
    }

    /// The status the host will serve.
    pub fn status(&self) -> u16 {
        self.status
    }

    /// The body the host will serialize, as JSON.
    pub fn body(&self) -> &Value {
        &self.body
    }

    /// The wire form: `{"status":200,"body":...}`.
    pub fn to_envelope(&self) -> String {
        json!({ "status": self.status, "body": self.body }).to_string()
    }
}

/// Decodes an envelope, turning a malformed one into a `400`.
///
/// A guest that panics on a bad request leaves the panel with a dead connection
/// and no explanation, so a decode failure is an answer like any other.
pub fn decode(request: &str) -> Result<Request, Reply> {
    serde_json::from_str::<Request>(request).map_err(|error| {
        Reply::bad_request(alloc::format!("the request is not an envelope: {error}"))
    })
}

pub mod http {
    use super::*;

    /// One HTTP anchor invocation.
    ///
    /// `path` is the path *below* the plugin's anchor prefix, already canonical:
    /// the host strips the prefix and normalizes the remainder, so a guest
    /// compares against the paths its manifest declares.
    #[derive(Debug, Deserialize)]
    pub struct HttpRequest {
        pub method: String,
        pub path: String,
        /// Query pairs, as the host parsed them. Present on every anchor; empty
        /// for `plugin call`.
        #[serde(default)]
        pub query: alloc::collections::BTreeMap<String, String>,
        /// The request body as text, or `None` when there was none.
        ///
        /// A string rather than JSON on purpose: an anchor may accept a body that
        /// is not JSON, and only the guest knows its own shape. [`Self::json_body`]
        /// is the parsed form for the common case.
        #[serde(default)]
        pub body: Option<String>,
    }

    impl HttpRequest {
        /// Parses the string body as JSON.
        ///
        /// `Ok(None)` when there is no body, and an error when there is one that
        /// this caller cannot parse — which is a `400`, not a `500`: the plugin is
        /// fine, the caller sent the wrong thing.
        pub fn json_body<T: serde::de::DeserializeOwned>(&self) -> Result<Option<T>, Reply> {
            match &self.body {
                None => Ok(None),
                Some(text) if text.trim().is_empty() => Ok(None),
                Some(text) => serde_json::from_str::<T>(text).map(Some).map_err(|error| {
                    Reply::bad_request(alloc::format!("the body is not JSON: {error}"))
                }),
            }
        }

        /// One query value, or `""`.
        pub fn query(&self, key: &str) -> Option<&str> {
            self.query.get(key).map(String::as_str)
        }
    }
}

pub use http::HttpRequest;
