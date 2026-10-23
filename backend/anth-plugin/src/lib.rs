//! The plugin contract for Anthracite Panel.
//!
//! A plugin declares its own vocabulary — [`Action`]s and the [`Response`]s they
//! produce — and the crate keeps every transport out of it. There are three
//! pipes and each has its own type:
//!
//! ```
//! use anth_plugin::{
//!     Action, CliRequest, Executor, HttpRequest, Plugin, Request, Response,
//! };
//! use serde_json::Value;
//!
//! const GREETING: Action = Action::new("greeting");
//! const COMMANDS: &[(&str, Action)] = &[("greet", GREETING)];
//!
//! #[derive(Default)]
//! struct Hello;
//!
//! impl Plugin for Hello {
//!     fn actions() -> &'static [Action] {
//!         &[GREETING]
//!     }
//!
//!     fn from_http(request: HttpRequest) -> Result<Request, Response> {
//!         match (request.method()?, request.path().as_str()) {
//!             (anth_plugin::Method::Get, "/greeting") => {
//!                 Ok(Request::new(GREETING, Value::Null))
//!             }
//!             _ => Err(Response::not_found("no such anchor")),
//!         }
//!     }
//!
//!     fn from_cli(request: CliRequest) -> Result<Request, Response> {
//!         request.into_request(COMMANDS)
//!     }
//!
//!     fn handle(&mut self, _executor: &mut dyn Executor, request: Request) -> Response {
//!         match request.action {
//!             GREETING => Response::message("hola"),
//!             _ => Response::not_found(request.action.as_str()),
//!         }
//!     }
//! }
//!
//! let response = Hello::from_http(HttpRequest {
//!     method: "GET".to_string(),
//!     path: "/greeting".to_string(),
//!     query: Default::default(),
//!     body: None,
//! })
//! .map(|request| Hello.handle(&mut NoHost, request));
//! assert!(response.is_ok());
//!
//! /// A host that grants nothing, which is enough for a greeting.
//! struct NoHost;
//! impl Executor for NoHost {
//!     fn exec(&mut self, _capability: &str, _args: &Value) -> Result<Value, String> {
//!         Err("nothing is granted".to_string())
//!     }
//!     fn log(&mut self, _message: &str) {}
//! }
//! ```
//!
//! Why three types instead of one envelope with a `kind` tag: a single envelope
//! invites one `match` that fabricates the other transport to reuse a handler,
//! and a fabricated request silently caps the CLI at whatever path it invented.
//! With one type per pipe, the plugin's mappers are the only place a transport
//! is read, and `Guest::handle` sees nothing but the plugin's own vocabulary.
//!
//! `plugin call` reaches the `direct` pipe, which has no mapper: the caller
//! sends `{"action": "...", "value": ...}` and the plugin's `handle` takes it as
//! is. That is what makes a plugin testable without either transport.

#![no_std]

extern crate alloc;

pub mod abi;
pub mod exec;
mod message;
pub mod plugin;
pub mod transport;

pub use abi::{
    ABI_VERSION, HostExecutor, alloc_string, answer_cli, answer_direct, answer_http, free_string,
    read_string, write_string,
};
pub use exec::Executor;
pub use message::{Action, Request, Response, Status};
pub use plugin::Plugin;
pub use transport::{CliRequest, HttpRequest, Method};

#[cfg(test)]
mod tests;
