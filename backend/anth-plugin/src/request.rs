//! What a plugin talks in: its own actions, its own answers.
//!
//! Neither type mentions a transport. An HTTP anchor, a CLI subcommand and a
//! direct `plugin call` all arrive as a [`Request`], because each of those pipes
//! has already paid its own translation in the plugin's mappers.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use serde_json::Value;

/// One operation a plugin answers, named by the plugin.
///
/// An action is a constant, not caller input, which is why it is built from a
/// `&'static str` and validated at compile time: a name derived from a request
/// would let a caller invent an operation the plugin never declared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Action(&'static str);

impl Action {
    /// Declares an action.
    ///
    /// # Panics
    ///
    /// At compile time, when used in a `const`, if the name is not a lowercase
    /// identifier. A plugin with a bad name fails to build rather than to run.
    pub const fn new(name: &'static str) -> Self {
        let bytes = name.as_bytes();
        assert!(!bytes.is_empty(), "an action name cannot be empty");
        assert!(
            bytes[0].is_ascii_lowercase(),
            "an action name starts with a lowercase letter"
        );
        let mut index = 1;
        while index < bytes.len() {
            let byte = bytes[index];
            assert!(
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'.' | b'_' | b'-'),
                "an action name holds only lowercase letters, digits, '.', '_' and '-'"
            );
            index += 1;
        }
        Self(name)
    }

    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

/// What the plugin was asked to do, and with what.
///
/// `value` is the plugin's own shape: the mappers decide it, so the crate never
/// guesses what a caller meant.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub action: Action,
    pub value: Value,
}

impl Request {
    pub fn new(action: Action, value: Value) -> Self {
        Self { action, value }
    }

    /// `value` as a type, or a `400` naming what did not fit.
    ///
    /// A caller sending the wrong shape is the caller's mistake, so it is an
    /// answer rather than a trap: the host reads a trap as a broken plugin.
    pub fn parse_value<T: serde::de::DeserializeOwned>(&self) -> Result<T, Response> {
        serde_json::from_value(self.value.clone()).map_err(|error| {
            Response::bad_request(alloc::format!(
                "`{}` expects a different value: {error}",
                self.action.as_str()
            ))
        })
    }
}

/// How a plugin classifies its answer.
///
/// The plugin names an outcome; the HTTP pipe turns it into a status code and
/// the CLI pipe into an exit code. Naming a code here would let a plugin serve a
/// `418`, and two pipes would then have to agree about a number neither owns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ok,
    Created,
    NoContent,
    BadRequest,
    Forbidden,
    NotFound,
    Conflict,
    Failed,
}

impl Status {
    /// The HTTP status the anchor pipe serves.
    ///
    /// Exhaustive on purpose: a new variant without a code does not compile.
    pub const fn code(self) -> u16 {
        match self {
            Status::Ok => 200,
            Status::Created => 201,
            Status::NoContent => 204,
            Status::BadRequest => 400,
            Status::Forbidden => 403,
            Status::NotFound => 404,
            Status::Conflict => 409,
            Status::Failed => 500,
        }
    }

    /// The exit code the CLI pipe returns, matching `anth-wire`'s convention:
    /// a bad request is `2`, everything else `1`. Success is `0`.
    pub const fn exit_code(self) -> u8 {
        match self {
            Status::Ok | Status::Created | Status::NoContent => 0,
            Status::BadRequest => 2,
            Status::Forbidden | Status::NotFound | Status::Conflict | Status::Failed => 1,
        }
    }
}

/// What the plugin answers, and how it classifies it.
#[derive(Debug, Clone, PartialEq)]
pub struct Response {
    pub status: Status,
    pub value: Value,
}

impl Response {
    pub fn new(status: Status, value: Value) -> Self {
        Self { status, value }
    }

    pub fn ok(value: Value) -> Self {
        Self::new(Status::Ok, value)
    }

    /// `200` with one `message`, the shape the panel's plugin widgets read.
    pub fn message(text: impl Into<String>) -> Self {
        Self::ok(serde_json::json!({ "message": text.into() }))
    }

    pub fn error(status: Status, detail: impl Into<String>) -> Self {
        Self::new(status, serde_json::json!({ "error": detail.into() }))
    }

    pub fn bad_request(detail: impl Into<String>) -> Self {
        Self::error(Status::BadRequest, detail)
    }

    pub fn forbidden(detail: impl Into<String>) -> Self {
        Self::error(Status::Forbidden, detail)
    }

    pub fn not_found(detail: impl Into<String>) -> Self {
        Self::error(Status::NotFound, detail)
    }

    pub fn conflict(detail: impl Into<String>) -> Self {
        Self::error(Status::Conflict, detail)
    }

    pub fn failed(detail: impl Into<String>) -> Self {
        Self::error(Status::Failed, detail)
    }

    /// Names the subcommands the plugin does have, from its own table.
    ///
    /// The message and the alias come from one source, so a subcommand cannot be
    /// routable without being listed here, or listed without being routable.
    pub fn unknown_command(name: &str, commands: &[(&str, Action)]) -> Self {
        let known: Vec<&str> = commands.iter().map(|(name, _)| *name).collect();
        Self::bad_request(alloc::format!(
            "no such subcommand `{name}`; try one of: {}",
            known.join(", ")
        ))
    }

    /// The wire form: `{"status":200,"body":...}`.
    ///
    /// `body` is omitted, not `null`, when the value is null: the host serves
    /// `204` only when `body` is absent, and a present `null` is served as the
    /// literal text `null`.
    pub fn to_envelope(&self) -> String {
        let status = self.status.code();
        match self.value {
            Value::Null => serde_json::json!({ "status": status }).to_string(),
            ref value => serde_json::json!({ "status": status, "body": value }).to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use serde_json::json;

    const GREETING: Action = Action::new("greeting");
    const ADD_PEER: Action = Action::new("add-peer");
    const NESTED: Action = Action::new("wg.show_dump");

    #[test]
    fn an_action_keeps_its_name() {
        assert_eq!(GREETING.as_str(), "greeting");
        assert_eq!(ADD_PEER.as_str(), "add-peer");
        assert_eq!(NESTED.as_str(), "wg.show_dump");
    }

    #[test]
    fn every_status_maps_to_an_http_code() {
        assert_eq!(Status::Ok.code(), 200);
        assert_eq!(Status::Created.code(), 201);
        assert_eq!(Status::NoContent.code(), 204);
        assert_eq!(Status::BadRequest.code(), 400);
        assert_eq!(Status::Forbidden.code(), 403);
        assert_eq!(Status::NotFound.code(), 404);
        assert_eq!(Status::Conflict.code(), 409);
        assert_eq!(Status::Failed.code(), 500);
    }

    /// The CLI convention this repeats lives in `anth-wire`: a malformed request
    /// is `2` so a script can tell "you asked wrong" from "it broke".
    #[test]
    fn exit_codes_follow_the_wire_convention() {
        assert_eq!(Status::Ok.exit_code(), 0);
        assert_eq!(Status::Created.exit_code(), 0);
        assert_eq!(Status::NoContent.exit_code(), 0);
        assert_eq!(Status::BadRequest.exit_code(), 2);
        assert_eq!(Status::NotFound.exit_code(), 1);
        assert_eq!(Status::Forbidden.exit_code(), 1);
        assert_eq!(Status::Conflict.exit_code(), 1);
        assert_eq!(Status::Failed.exit_code(), 1);
    }

    /// The keys are compared, not the text: a JSON object has no order, and
    /// `serde_json` writes them sorted. Pinning the string would fail on a
    /// harmless reordering and hide a real change.
    fn envelope(status: &Response) -> Value {
        serde_json::from_str(&status.to_envelope()).expect("the envelope is JSON")
    }

    #[test]
    fn a_response_is_the_envelope_the_host_reads() {
        assert_eq!(
            envelope(&Response::ok(json!("anchored"))),
            json!({ "status": 200, "body": "anchored" })
        );
    }

    /// A `null` value must drop `body` entirely: the host serves `204` only when
    /// the key is absent, and serves a present `null` as the text `null`.
    #[test]
    fn a_null_value_omits_the_body() {
        assert_eq!(
            envelope(&Response::new(Status::NoContent, Value::Null)),
            json!({ "status": 204 })
        );
    }

    #[test]
    fn an_error_carries_its_detail() {
        assert_eq!(
            envelope(&Response::failed("no binary")),
            json!({ "status": 500, "body": { "error": "no binary" } })
        );
    }

    #[test]
    fn an_unknown_command_lists_the_known_ones() {
        let response =
            Response::unknown_command("frobnicate", &[("status", GREETING), ("peers", ADD_PEER)]);

        assert_eq!(response.status, Status::BadRequest);
        assert_eq!(
            response.value["error"],
            json!("no such subcommand `frobnicate`; try one of: status, peers")
        );
    }

    #[test]
    fn a_mismatched_value_is_a_400_naming_the_action() {
        let request = Request::new(ADD_PEER, json!({ "interface": 7 }));

        let response = request.parse_value::<Peer>().unwrap_err();

        assert_eq!(response.status, Status::BadRequest);
        assert!(
            response.value["error"]
                .as_str()
                .unwrap()
                .contains("add-peer"),
            "the message must name the action: {}",
            response.value
        );
    }

    #[derive(serde::Deserialize, Debug)]
    struct Peer {
        #[allow(dead_code)]
        interface: String,
    }
}
