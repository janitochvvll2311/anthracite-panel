//! Read-only WireGuard reference plugin.

pub mod logic;

use anth_plugin::{Action, CliRequest, Executor, HttpRequest, Method, Plugin, Request, Response};

pub const INTERFACES: Action = Action::new("interfaces");
pub const SHOW: Action = Action::new("show");
pub const ACTIONS: &[Action] = &[INTERFACES, SHOW];
pub const COMMANDS: &[(&str, Action)] = &[("interfaces", INTERFACES), ("show", SHOW)];

#[derive(Default)]
pub struct WireGuard;

impl Plugin for WireGuard {
    fn actions() -> &'static [Action] {
        ACTIONS
    }

    fn from_http(request: HttpRequest) -> Result<Request, Response> {
        match (request.method()?, request.path().as_str()) {
            (Method::Get, "/interfaces") => Ok(Request::new(INTERFACES, serde_json::Value::Null)),
            (Method::Post, "/show") => {
                let body = request
                    .body_json::<serde_json::Value>()?
                    .unwrap_or(serde_json::Value::Null);
                if logic::interface_of(&body).is_none() {
                    return Err(Response::bad_request("the body needs an `interface` name"));
                }
                Ok(Request::new(SHOW, body))
            }
            (method, path) => Err(Response::not_found(format!(
                "the WireGuard plugin has no `{method} {path}`"
            ))),
        }
    }

    fn from_cli(request: CliRequest) -> Result<Request, Response> {
        request.into_request(COMMANDS)
    }

    fn handle(&mut self, executor: &mut dyn Executor, request: Request) -> Response {
        match request.action {
            INTERFACES => logic::interfaces(executor),
            SHOW => match logic::interface_of(&request.value) {
                Some(interface) => logic::show(executor, interface),
                None => Response::bad_request("name one interface, for example `wg0`"),
            },
            other => Response::not_found(format!("no action `{}`", other.as_str())),
        }
    }
}

anth_plugin::anth_plugin!(WireGuard);
