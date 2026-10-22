//! The capability seam, and the route table built on it.
//!
//! A guest never runs a program. It asks for a *capability* the manifest froze at
//! install time and the host renders the argv for. [`Executor`] is that seam: the
//! wasm build implements it over the imported `anth_exec`, a test implements it
//! over a table of canned answers, and neither the route table nor the plugin's
//! own logic knows which one it got.

use alloc::string::String;
use alloc::vec::Vec;

use serde_json::Value;

use crate::command::Command;
use crate::envelope::Reply;

/// One capability call.
///
/// Arguments go in as a JSON **object**: the host decodes them into a map of
/// strings before rendering the argv, so `null` is not "no arguments", it is a
/// decode failure. Pass `{}` for a capability that takes none.
pub trait Executor {
    /// Runs one frozen capability and returns its answer.
    ///
    /// The answer is the host's JSON object — at least `stdout` — and a refusal
    /// is an `Err` carrying the daemon's detail. A refusal is never swallowed: a
    /// plugin that cannot reach its program still has to tell the operator why.
    fn exec(&mut self, capability: &str, args: &Value) -> Result<Value, String>;

    /// Writes one line to the daemon log.
    ///
    /// The log belongs to the operator, not to the HTTP caller, so this is for
    /// diagnostics. A plugin cannot put text into its caller's response this way.
    fn log(&mut self, message: &str);

    /// A convenience for the common case: run a capability with no arguments.
    ///
    /// This exists so `&Value::Null` is never written by hand. The host decodes
    /// arguments into an object, so `Null` is refused while `{}` succeeds;
    /// routing the no-argument case through one method is what stops that
    /// mistake from being repeated per plugin.
    fn exec_bare(&mut self, capability: &str) -> Result<Value, String> {
        self.exec(capability, &serde_json::json!({}))
    }
}

/// One route a plugin answers.
///
/// The handler sees a [`Command`], so the same function serves an HTTP anchor and
/// a `plugin call`: the transport is folded away before dispatch, and a plugin
/// never writes a second table for the CLI.
pub type Handler = fn(&mut dyn Executor, &Command) -> Reply;

/// A set of routes, matched by method and path.
///
/// The table is what a plugin author fills in; [`Router::dispatch`] is the part
/// they would otherwise rewrite. Matching is exact: a route declares the path its
/// manifest declares, and the host has already stripped its own prefix, so
/// there is no prefix arithmetic left to get wrong.
#[derive(Default)]
pub struct Router {
    routes: Vec<Route>,
}

struct Route {
    method: Method,
    path: &'static str,
    handler: Handler,
}

/// The HTTP methods a plugin route may declare.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

impl Method {
    /// Parses a method word, case-insensitively.
    ///
    /// Public because a CLI call names its method in its first `argv` item, so
    /// [`crate::Command`] has to recognise one to know whether it was given.
    pub fn parse(text: &str) -> Option<Self> {
        match text.to_ascii_uppercase().as_str() {
            "GET" => Some(Self::Get),
            "POST" => Some(Self::Post),
            "PUT" => Some(Self::Put),
            "PATCH" => Some(Self::Patch),
            "DELETE" => Some(Self::Delete),
            _ => None,
        }
    }

    fn matches(self, text: &str) -> bool {
        Method::parse(text) == Some(self)
    }

    /// The wire spelling, for the `404` and for [`crate::Command`].
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
        }
    }
}

impl Router {
    pub fn new() -> Self {
        Self::default()
    }

    /// Declares one route.
    ///
    /// The path must be the one the manifest declares, leading slash included.
    pub fn route(mut self, method: Method, path: &'static str, handler: Handler) -> Self {
        self.routes.push(Route {
            method,
            path,
            handler,
        });
        self
    }

    /// `GET <path>`.
    pub fn get(self, path: &'static str, handler: Handler) -> Self {
        self.route(Method::Get, path, handler)
    }

    /// `POST <path>`.
    pub fn post(self, path: &'static str, handler: Handler) -> Self {
        self.route(Method::Post, path, handler)
    }

    /// Answers one command, or a `404` naming what was asked for.
    ///
    /// A `404` rather than a panic: an unknown route is a caller mistake the panel
    /// should display. The command is echoed so the operator can see which anchor
    /// the manifest is missing, and the same table answers both transports.
    pub fn dispatch(&self, executor: &mut dyn Executor, command: &Command) -> Reply {
        for route in &self.routes {
            if route.path == command.path() && route.method.matches(command.method()) {
                return (route.handler)(executor, command);
            }
        }
        Reply::not_found(alloc::format!(
            "no route for {} {}",
            command.method(),
            command.path()
        ))
    }
}
