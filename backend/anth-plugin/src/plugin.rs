//! The trait a plugin implements: three mappers and the behaviour they feed.
//!
//! The mappers have no default on purpose. A default would let a plugin skip a
//! transport and have the library pick one for it, which is exactly the
//! mistake this contract exists to prevent: a route the plugin implements but
//! nobody can reach.
//!
//! [`Plugin::http`], [`Plugin::cli`] and [`Plugin::direct`] are the glue the
//! library owns, so no plugin has to rewrite the same `match` on a `Result`.

use crate::exec::Executor;
use crate::request::{Request, Response};
use crate::transport::{CliRequest, HttpRequest};

pub trait Plugin {
    /// Every action this plugin answers.
    ///
    /// The table is the plugin's whole vocabulary, and it is what lets the
    /// `direct` pipe name an action in JSON: an `Action` is a `&'static str`, so
    /// a name arriving at run time can only become an `Action` by being found
    /// here. A name outside this table is a `404`, never an invented action.
    fn actions() -> &'static [crate::request::Action];

    /// HTTP anchor to the plugin's vocabulary.
    ///
    /// `Err` is the answer: the mapper decides whether an unknown path is a
    /// `404` and a bad body a `400`, because only the plugin knows which.
    fn from_http(request: HttpRequest) -> Result<Request, Response>;

    /// A CLI command line to the plugin's vocabulary.
    ///
    /// [`CliRequest`] holds the line as text; splitting it into words and
    /// resolving the subcommand against the plugin's alias table is
    /// [`CliRequest::into_request`] or the finer accessors around it.
    fn from_cli(request: CliRequest) -> Result<Request, Response>;

    /// The plugin's vocabulary, which is the plugin.
    fn handle(&mut self, executor: &mut dyn Executor, request: Request) -> Response;

    /// Serves one HTTP anchor: map, then handle. A refused map is the answer.
    fn http(&mut self, executor: &mut dyn Executor, request: HttpRequest) -> Response {
        match Self::from_http(request) {
            Ok(request) => self.handle(executor, request),
            Err(response) => response,
        }
    }

    /// Serves one CLI invocation: map, then handle.
    fn cli(&mut self, executor: &mut dyn Executor, request: CliRequest) -> Response {
        match Self::from_cli(request) {
            Ok(request) => self.handle(executor, request),
            Err(response) => response,
        }
    }

    /// Serves a `Request` that is already in the plugin's vocabulary.
    ///
    /// This pipe has no mapper because there is nothing to map: `plugin call`
    /// can hand a plugin exactly what its own `handle` takes, which is what makes
    /// a plugin testable without either transport.
    fn direct(&mut self, executor: &mut dyn Executor, request: Request) -> Response {
        self.handle(executor, request)
    }
}
