//! The hello plugin's wasm entry point.
//!
//! Behaviour lives in [`logic`], which is plain Rust: it names a capability
//! through [`anth_plugin::Executor`] and never touches an import, a pointer or
//! the allocator, so it runs as a normal test on the host. This file is the three
//! mappers the contract asks for, one per transport, and the action table they
//! resolve against.

pub mod logic;

use anth_plugin::{
    Action, CliRequest, Executor, HttpRequest, Method, Plugin, Request, Response, Status,
};
use serde_json::Value;

/// The actions this plugin answers.
pub const GREETING: Action = Action::new("greeting");
pub const STATUS: Action = Action::new("status");

/// Every action, which is what `plugin call` resolves a name against.
pub const ACTIONS: &[Action] = &[GREETING, STATUS];

/// The subcommands the CLI may use, and the action each one means.
///
/// One table feeds both the routing and the "try one of" message, so a
/// subcommand cannot be routable without being advertised.
pub const COMMANDS: &[(&str, Action)] = &[("greet", GREETING), ("status", STATUS)];

/// The guest the host calls.
#[derive(Default)]
pub struct Hello;

impl Plugin for Hello {
    fn actions() -> &'static [Action] {
        ACTIONS
    }

    fn from_http(request: HttpRequest) -> Result<Request, Response> {
        match (request.method()?, request.path().as_str()) {
            (Method::Get, logic::GREETING_PATH) => Ok(Request::new(GREETING, Value::Null)),
            (Method::Get, logic::STATUS_PATH) => Ok(Request::new(STATUS, Value::Null)),
            (method, path) => Err(Response::not_found(format!(
                "the hello plugin has no `{method} {path}`"
            ))),
        }
    }

    fn from_cli(request: CliRequest) -> Result<Request, Response> {
        request.args()?.into_request(COMMANDS)
    }

    fn handle(&mut self, executor: &mut dyn Executor, request: Request) -> Response {
        match request.action {
            GREETING => logic::greeting(executor),
            STATUS => logic::status(&request.value),
            other => Response::new(
                Status::NotFound,
                serde_json::json!({ "error": format!("no action `{}`", other.as_str()) }),
            ),
        }
    }
}

anth_plugin::anth_plugin!(Hello);
