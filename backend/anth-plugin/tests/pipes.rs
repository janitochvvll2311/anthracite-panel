//! The contract exercised through the three pipes, end to end.
//!
//! These run on the host: `answer_http` and its siblings take a `&str` and
//! return a `String`, so nothing here needs a wasm runtime. Every assertion goes
//! through a real mapper and a real `handle`.

use anth_plugin::{Action, CliRequest, Executor, HttpRequest, Plugin, Request, Response, Status};
use serde_json::{Value, json};

const GREETING: Action = Action::new("greeting");
const STATUS: Action = Action::new("status");
const ADD_PEER: Action = Action::new("add-peer");
const COMMANDS: &[(&str, Action)] = &[("greet", GREETING), ("status", STATUS)];
const ACTIONS: &[Action] = &[GREETING, STATUS, ADD_PEER];

/// A host that grants the one capability this plugin uses.
struct Fake;

impl Executor for Fake {
    fn exec(&mut self, _capability: &str, _args: &Value) -> Result<Value, String> {
        Ok(json!({ "stdout": "turing\n" }))
    }

    fn log_at(&mut self, _level: anth_plugin::LogLevel, _message: &str) {}
}

#[derive(Default)]
struct Hello;

impl Plugin for Hello {
    fn actions() -> &'static [Action] {
        ACTIONS
    }

    fn from_http(request: HttpRequest) -> Result<Request, Response> {
        match (request.method()?, request.path().as_str()) {
            (anth_plugin::Method::Get, "/greeting") => Ok(Request::new(GREETING, Value::Null)),
            (anth_plugin::Method::Get, "/status") => Ok(Request::new(
                STATUS,
                json!({ "verbose": request.query("verbose").is_some() }),
            )),
            (anth_plugin::Method::Post, "/peers") => match request.body_json::<Value>()? {
                Some(body) => Ok(Request::new(ADD_PEER, body)),
                None => Err(Response::bad_request("`POST /peers` needs a body")),
            },
            (_, path) => Err(Response::not_found(path)),
        }
    }

    fn from_cli(request: CliRequest) -> Result<Request, Response> {
        // `--json` is a modifier rather than an action, so the mapper reads it
        // itself instead of taking the whole line as one alias.
        if request.flag("json") {
            let action = request.action(COMMANDS)?;
            return Ok(Request::new(action, json!({ "format": "json" })));
        }
        request.into_request(COMMANDS)
    }

    fn handle(&mut self, executor: &mut dyn Executor, request: Request) -> Response {
        match request.action {
            GREETING => match executor.exec_bare("host.hostname") {
                Ok(reply) => {
                    let hostname = reply
                        .get("stdout")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown")
                        .trim();
                    Response::message(format!("Hola desde {hostname}"))
                }
                Err(detail) => Response::failed(detail),
            },
            STATUS => Response::ok(json!({ "state": "ready", "value": request.value })),
            ADD_PEER => Response::new(Status::Created, json!({ "added": request.value })),
            other => Response::not_found(other.as_str()),
        }
    }
}

fn http(payload: &str) -> Value {
    let text = anth_plugin::answer_http(&mut Hello, &mut Fake, payload);
    envelope(&text)
}

fn cli(payload: &str) -> Value {
    let text = anth_plugin::answer_cli(&mut Hello, &mut Fake, payload);
    envelope(&text)
}

/// The same call, written as the host writes it: a JSON string of the line.
fn cli_line(line: &str) -> Value {
    cli(&serde_json::to_string(line).expect("a line encodes as a JSON string"))
}

fn direct(payload: &str) -> Value {
    let text = anth_plugin::answer_direct(&mut Hello, &mut Fake, payload);
    envelope(&text)
}

fn envelope(text: &str) -> Value {
    serde_json::from_str(text).expect("the answer is an envelope")
}

#[test]
fn an_http_anchor_reaches_the_plugin_through_its_mapper() {
    let answer = http(r#"{"method":"GET","path":"/greeting","query":{},"body":null}"#);

    assert_eq!(answer["status"], json!(200));
    assert_eq!(answer["body"]["message"], json!("Hola desde turing"));
}

/// axum hands the path over without its leading slash, so a mapper comparing
/// against the manifest's spelling works only because `path()` normalizes it.
#[test]
fn an_http_path_without_its_slash_still_matches() {
    let answer = http(r#"{"method":"GET","path":"status","query":{},"body":null}"#);

    assert_eq!(answer["status"], json!(200));
    assert_eq!(answer["body"]["state"], json!("ready"));
}

#[test]
fn an_http_query_reaches_the_value() {
    let answer = http(r#"{"method":"GET","path":"/status","query":{"verbose":"1"},"body":null}"#);

    assert_eq!(answer["body"]["value"]["verbose"], json!(true));
}

#[test]
fn an_http_body_reaches_the_value() {
    let answer =
        http(r#"{"method":"POST","path":"/peers","query":{},"body":"{\"interface\":\"wg0\"}"}"#);

    assert_eq!(
        answer["status"],
        json!(201),
        "the mapper's status is served"
    );
    assert_eq!(answer["body"]["added"]["interface"], json!("wg0"));
}

/// A body that is not JSON is refused by the mapper, which is the only part
/// that knows whether this anchor speaks JSON.
#[test]
fn an_http_body_that_is_not_json_is_a_400() {
    let answer = http(r#"{"method":"POST","path":"/peers","query":{},"body":"not json"}"#);

    assert_eq!(answer["status"], json!(400));
}

#[test]
fn an_unknown_http_path_is_the_mappers_404() {
    let answer = http(r#"{"method":"GET","path":"/nope","query":{},"body":null}"#);

    assert_eq!(answer["status"], json!(404));
    assert_eq!(answer["body"]["error"], json!("/nope"));
}

#[test]
fn an_unknown_http_method_is_a_400() {
    let answer = http(r#"{"method":"TRACE","path":"/status","query":{},"body":null}"#);

    assert_eq!(answer["status"], json!(400));
}

#[test]
fn a_cli_alias_reaches_the_action_it_names() {
    let answer = cli_line("greet");

    assert_eq!(answer["status"], json!(200));
    assert_eq!(answer["body"]["message"], json!("Hola desde turing"));
}

#[test]
fn a_cli_flag_reaches_the_value() {
    let answer = cli_line("status --verbose");

    assert_eq!(answer["status"], json!(200));
    assert_eq!(answer["body"]["value"]["flags"]["verbose"], json!(true));
}

/// A mapper can read a word itself without losing the alias resolution, which is
/// what the loose accessors are for.
#[test]
fn a_mapper_can_read_one_flag_itself() {
    let answer = cli_line("status --json");

    assert_eq!(answer["status"], json!(200));
    assert_eq!(answer["body"]["value"]["format"], json!("json"));
}

/// The point of one alias table: a subcommand the plugin does not declare is
/// refused with the ones it does, and the list cannot drift from the routing.
#[test]
fn an_unknown_cli_subcommand_lists_the_known_ones() {
    let answer = cli_line("frobnicate");

    assert_eq!(answer["status"], json!(400));
    assert_eq!(
        answer["body"]["error"],
        json!("no such subcommand `frobnicate`; try one of: greet, status")
    );
}

/// The entry models a command line, so the payload is a JSON string. An object
/// is what the previous contract sent and must be a `400`, not a command named
/// after the object.
#[test]
fn a_cli_payload_that_is_not_a_string_is_a_400() {
    let answer = cli(r#"{"argv":["status"]}"#);

    assert_eq!(answer["status"], json!(400));
    assert!(
        answer["body"]["error"]
            .as_str()
            .unwrap()
            .contains("JSON string"),
        "got: {}",
        answer["body"]
    );
}

/// A quoted argument survives the round trip, which is the whole reason the line
/// is text and not a list the host already split.
#[test]
fn a_quoted_cli_argument_keeps_its_space() {
    let answer = cli_line(r#"greet --name "Ana Sol""#);

    assert_eq!(answer["status"], json!(200));
}

#[test]
fn an_empty_cli_argv_is_a_400() {
    assert_eq!(cli_line("")["status"], json!(400));
}

/// `plugin call` reaches the plugin with no mapper at all: the caller already
/// speaks the plugin's vocabulary.
#[test]
fn the_direct_pipe_needs_no_mapper() {
    let answer = direct(r#"{"action":"status","value":{"verbose":true}}"#);

    assert_eq!(answer["status"], json!(200));
    assert_eq!(answer["body"]["value"]["verbose"], json!(true));
}

#[test]
fn the_direct_pipe_can_run_an_action_no_transport_exposes() {
    let answer = direct(r#"{"action":"add-peer","value":{"interface":"wg0"}}"#);

    assert_eq!(answer["status"], json!(201));
    assert_eq!(answer["body"]["added"]["interface"], json!("wg0"));
}

/// An action is a `&'static str`, so a name from a payload can only become one
/// through the plugin's own table. Anything else is refused, never fabricated.
#[test]
fn an_action_outside_the_table_is_a_404() {
    let answer = direct(r#"{"action":"drop-database","value":null}"#);

    assert_eq!(answer["status"], json!(404));
    assert_eq!(answer["body"]["error"], json!("no action `drop-database`"));
}

#[test]
fn a_direct_payload_that_is_not_a_request_is_a_400() {
    assert_eq!(direct("not json")["status"], json!(400));
    assert_eq!(direct(r#"{"value":1}"#)["status"], json!(400));
}

/// Every pipe lands in the same `handle`, so the same action is reachable from
/// all three. A pipe that could not reach it would be a route only one transport
/// can serve.
///
/// The `value` is *not* compared, and that is the point of the design: each
/// mapper hands `handle` what its own transport actually carried, so an HTTP
/// `?verbose` and a CLI `--verbose` are two truths about two different calls.
/// Requiring them to be identical would force the mappers to invent a shape the
/// caller never sent.
#[test]
fn all_three_pipes_reach_the_same_action() {
    let via_http = http(r#"{"method":"GET","path":"/status","query":{},"body":null}"#);
    let via_cli = cli_line("status");
    let via_direct = direct(r#"{"action":"status","value":null}"#);

    for (pipe, answer) in [
        ("http", &via_http),
        ("cli", &via_cli),
        ("direct", &via_direct),
    ] {
        assert_eq!(
            answer["status"],
            json!(200),
            "{pipe} did not reach the action"
        );
        assert_eq!(
            answer["body"]["state"],
            json!("ready"),
            "{pipe} answered another action"
        );
    }
}

/// The exact wire shape `v1`'s anchor test asserts, so a change to the envelope
/// fails here rather than in production.
#[test]
fn the_envelope_is_the_one_the_host_reads() {
    let answer = anth_plugin::answer_http(
        &mut Hello,
        &mut Fake,
        r#"{"method":"GET","path":"/greeting","query":{},"body":null}"#,
    );

    let parsed: Value = serde_json::from_str(&answer).expect("the answer is JSON");
    assert_eq!(parsed["status"], json!(200));
    assert_eq!(
        parsed["body"],
        json!({ "message": "Hola desde turing" }),
        "the host reads `status` and `body`"
    );
}

/// A capability refused by the host is a `500` carrying the detail, not a `200`
/// with an empty name.
#[test]
fn a_refused_capability_is_a_500() {
    struct Refusing;

    impl Executor for Refusing {
        fn exec(&mut self, _capability: &str, _args: &Value) -> Result<Value, String> {
            Err("`hostname` is not installed".to_string())
        }

        fn log_at(&mut self, _level: anth_plugin::LogLevel, _message: &str) {}
    }

    let response = Hello.handle(&mut Refusing, Request::new(GREETING, Value::Null));

    assert_eq!(response.status, Status::Failed);
    assert_eq!(
        response.value["error"],
        json!("`hostname` is not installed")
    );
}
