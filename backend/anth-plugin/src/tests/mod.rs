//! The contract's own tests.
//!
//! These pin the wire shapes the host depends on. A change here that the host
//! does not also make is a plugin that loads and answers the wrong thing, so each
//! test names the host file it mirrors.

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};

use serde_json::{Value, json};

use crate::command::Command;
use crate::envelope::{HttpRequest, Reply, Request, decode};
use crate::exec::{Executor, Router};

/// The envelope `plugins::api::anchor` builds, verbatim from its `json!` call.
#[test]
fn decodes_the_anchor_envelope_the_host_sends() {
    let text = r#"{"kind":"http","method":"GET","path":"/greeting","query":{},"body":null}"#;
    match decode(text).expect("the host envelope must decode") {
        Request::Http(request) => {
            assert_eq!(request.method, "GET");
            assert_eq!(request.path, "/greeting");
            assert!(request.query.is_empty());
            assert!(request.body.is_none());
        }
        Request::Cli(_) => panic!("an anchor is not a CLI call"),
    }
}

/// `body` travels as a string, because an anchor is not required to speak JSON.
#[test]
fn a_body_is_a_string_not_parsed_json() {
    let text =
        r#"{"kind":"http","method":"POST","path":"/run","query":{},"body":"{\"job\":\"x\"}"}"#;
    let request = match decode(text).expect("decodes") {
        Request::Http(request) => request,
        Request::Cli(_) => panic!("wrong kind"),
    };
    assert_eq!(request.body.as_deref(), Some(r#"{"job":"x"}"#));

    #[derive(serde::Deserialize)]
    struct Run {
        job: String,
    }
    let parsed: Run = request
        .json_body()
        .expect("the string body is JSON")
        .expect("present");
    assert_eq!(parsed.job, "x");
}

/// A body that is not JSON is a `400` for the caller, not a `500` for the plugin.
#[test]
fn a_non_json_body_is_a_client_error() {
    let request = HttpRequest {
        method: "POST".to_string(),
        path: "/run".to_string(),
        query: BTreeMap::new(),
        body: Some("not json".to_string()),
    };
    let reply = request.json_body::<Value>().unwrap_err();
    assert_eq!(reply.status(), 400);
}

#[test]
fn an_empty_body_is_not_an_error() {
    for body in [None, Some(String::new()), Some("   ".to_string())] {
        let request = HttpRequest {
            method: "POST".to_string(),
            path: "/run".to_string(),
            query: BTreeMap::new(),
            body,
        };
        let parsed: Option<Value> = request.json_body().expect("no body is fine");
        assert!(parsed.is_none());
    }
}

/// `plugin call <id> '<json>'` sends this shape through untouched.
#[test]
fn decodes_the_cli_envelope() {
    let text = r#"{"kind":"cli","argv":["/status","--short"]}"#;
    match decode(text).expect("decodes") {
        Request::Cli(call) => assert_eq!(call.argv, ["/status", "--short"]),
        Request::Http(_) => panic!("a CLI call is not an anchor"),
    }
}

/// A malformed envelope answers `400` instead of trapping: a trap reads as a
/// broken plugin and tells the caller nothing.
#[test]
fn a_malformed_envelope_is_a_400_not_a_panic() {
    let reply = decode("not an envelope").unwrap_err();
    assert_eq!(reply.status(), 400);
    assert!(
        reply.body()["error"]
            .as_str()
            .unwrap()
            .contains("not an envelope")
    );

    let unknown = decode(r#"{"kind":"telepathy"}"#).unwrap_err();
    assert_eq!(unknown.status(), 400);
}

#[test]
fn replies_carry_the_status_and_body_the_host_reads() {
    let reply = Reply::message("hola");
    assert_eq!(reply.status(), 200);
    assert_eq!(reply.body()["message"], json!("hola"));

    let envelope: Value = serde_json::from_str(&reply.to_envelope()).unwrap();
    assert_eq!(envelope["status"], json!(200));
    assert_eq!(envelope["body"]["message"], json!("hola"));
}

#[test]
fn named_failures_map_to_the_statuses_the_host_serves() {
    assert_eq!(Reply::bad_request("x").status(), 400);
    assert_eq!(Reply::not_found("x").status(), 404);
    assert_eq!(Reply::failed("x").status(), 500);
    assert_eq!(Reply::error(409, "x").status(), 409);
}

#[test]
fn a_failure_body_is_an_error_key() {
    let reply = Reply::failed("the host refused");
    assert_eq!(reply.body()["error"], json!("the host refused"));
}

/// Builds a `Command` the way the HTTP transport does.
fn http_command(method: &str, path: &str) -> Command {
    Command::new(method, path, BTreeMap::new(), json!({}))
}

/// A route table answers by method and path, and `404`s the rest.
#[test]
fn the_router_matches_method_and_path_exactly() {
    fn hit(_: &mut dyn Executor, _: &Command) -> Reply {
        Reply::message("hit")
    }

    let router = Router::new().get("/status", hit).post("/run", hit);

    let ask = |method: &str, path: &str| {
        let command = http_command(method, path);
        router.dispatch(&mut Refusing, &command)
    };

    assert_eq!(ask("GET", "/status").status(), 200);
    assert_eq!(
        ask("get", "/status").status(),
        200,
        "the method is case-insensitive"
    );
    assert_eq!(ask("POST", "/run").status(), 200);
    assert_eq!(
        ask("GET", "/run").status(),
        404,
        "the wrong method is not a match"
    );
    assert_eq!(
        ask("GET", "/statu").status(),
        404,
        "matching is exact, not a prefix"
    );
    assert_eq!(
        ask("GET", "/status/").status(),
        404,
        "a trailing slash is a different path"
    );
}

/// The path appears in the `404` so an operator can see which anchor is missing.
#[test]
fn a_missing_route_names_what_was_asked_for() {
    let router = Router::new();
    let reply = router.dispatch(&mut Refusing, &http_command("delete", "/nope"));
    let text = reply.body()["error"].as_str().unwrap();
    assert!(text.contains("/nope"), "got {text}");
    assert!(
        text.contains("DELETE"),
        "the method is uppercased: got {text}"
    );
}

/// `exec_bare` passes an object, because the host decodes arguments into a map.
///
/// The host's `anth_exec` parses arguments as `BTreeMap<String, String>`, so
/// `null` is a decode failure while `{}` succeeds. This test is the reason the
/// convenience method exists: hand-written `&Value::Null` is a plugin whose every
/// capability call is refused.
#[test]
fn exec_bare_passes_an_object_not_null() {
    let mut executor = Recording::default();
    let _ = executor.exec_bare("host.hostname");

    let arguments = executor.seen.first().expect("one call").1.clone();
    assert!(
        arguments.is_object(),
        "arguments must be an object, got {arguments}"
    );
    assert_eq!(arguments, json!({}));

    // And the failure this prevents, pinned as evidence.
    let null_is_not_an_object = !Value::Null.is_object();
    assert!(null_is_not_an_object);
    let decoded = serde_json::from_str::<BTreeMap<String, String>>(&Value::Null.to_string());
    assert!(
        decoded.is_err(),
        "null must not decode into the host's argument map"
    );
}

/// A capability refusal reaches the plugin as an `Err` carrying the host's detail.
#[test]
fn a_refusal_is_an_error_not_a_value() {
    let mut executor = Recording {
        refused: Some("`hostname` is not installed".to_string()),
        ..Recording::default()
    };
    let error = executor.exec_bare("host.hostname").unwrap_err();
    assert!(error.contains("not installed"), "got {error}");
}

#[derive(Default)]
struct Recording {
    seen: alloc::vec::Vec<(String, Value)>,
    refused: Option<String>,
}

impl Executor for Recording {
    fn exec(&mut self, capability: &str, args: &Value) -> Result<Value, String> {
        self.seen.push((capability.to_string(), args.clone()));
        match &self.refused {
            Some(detail) => Err(detail.clone()),
            None => Ok(json!({"ok": true, "stdout": "box", "stderr": ""})),
        }
    }

    fn log(&mut self, _: &str) {}
}

/// An executor that fails the test if a handler tries to reach the host.
struct Refusing;

impl Executor for Refusing {
    fn exec(&mut self, capability: &str, _: &Value) -> Result<Value, String> {
        panic!("no capability should run here: {capability}");
    }
    fn log(&mut self, _: &str) {}
}

/// `answer` decodes, hands to the guest and encodes; a bad envelope never reaches
/// the guest, so a plugin cannot be blamed for a caller's mistake.
#[test]
fn answer_wraps_the_round_trip_and_short_circuits_a_bad_envelope() {
    #[derive(Default)]
    struct Echo {
        calls: usize,
    }

    impl crate::Guest for Echo {
        fn handle(&mut self, command: Command) -> Reply {
            self.calls += 1;
            Reply::message(command.path())
        }
    }

    let mut guest = Echo::default();
    let reply: Value = serde_json::from_str(&crate::answer(
        &mut guest,
        r#"{"kind":"http","method":"GET","path":"/a"}"#,
    ))
    .unwrap();
    assert_eq!(reply["status"], json!(200));
    assert_eq!(reply["body"]["message"], json!("/a"));
    assert_eq!(guest.calls, 1);

    let refused: Value = serde_json::from_str(&crate::answer(&mut guest, "nonsense")).unwrap();
    assert_eq!(refused["status"], json!(400));
    assert_eq!(
        guest.calls, 1,
        "the guest is never called for a bad envelope"
    );
}

// ---------------------------------------------------------------------------
// One command for both transports
// ---------------------------------------------------------------------------

/// An HTTP anchor folds into method, path, query and a parsed input.
#[test]
fn an_http_envelope_becomes_a_command() {
    let text = r#"{"kind":"http","method":"get","path":"/run","query":{"page":"2"},
        "body":"{\"job\":\"x\"}"}"#;
    let command = decode(text)
        .expect("decodes")
        .into_command()
        .expect("folds");

    assert_eq!(command.method(), "GET", "the method is uppercased");
    assert_eq!(command.path(), "/run");
    assert_eq!(command.query("page"), Some("2"));
    assert_eq!(command.input()["job"], json!("x"), "a JSON body is parsed");
    assert!(
        command.args().is_empty(),
        "an HTTP call has no positional arguments"
    );
}

/// A body that is not JSON is kept as a string rather than refused: an anchor is
/// not required to speak JSON, and only the plugin knows its own shape.
#[test]
fn a_non_json_anchor_body_is_kept_as_a_string() {
    let text = r#"{"kind":"http","method":"POST","path":"/run","query":{},"body":"plain text"}"#;
    let command = decode(text)
        .expect("decodes")
        .into_command()
        .expect("folds");
    assert_eq!(command.input(), &json!("plain text"));
}

/// No body is `{}`, so a handler never distinguishes "absent" from "empty".
#[test]
fn an_absent_body_is_an_empty_object() {
    for body in [r#"null"#, r#""""#, r#""   ""#] {
        let text = alloc::format!(
            r#"{{"kind":"http","method":"GET","path":"/x","query":{{}},"body":{body}}}"#
        );
        let command = decode(&text)
            .expect("decodes")
            .into_command()
            .expect("folds");
        assert_eq!(command.input(), &json!({}), "body {body}");
    }
}

/// The point of the fold: `plugin call <id> '/status'` is the same route the
/// browser reaches, so the CLI extends to every anchor a plugin declares instead
/// of a single default one.
#[test]
fn a_cli_path_reaches_the_same_route_as_an_anchor() {
    fn status(_: &mut dyn Executor, _: &Command) -> Reply {
        Reply::message("ok")
    }
    fn run(_: &mut dyn Executor, command: &Command) -> Reply {
        Reply::message(command.args().join(","))
    }

    let router = Router::new().get("/status", status).post("/run", run);

    let cli = decode(r#"{"kind":"cli","argv":["/status"]}"#)
        .expect("decodes")
        .into_command()
        .expect("folds");
    assert_eq!(router.dispatch(&mut Refusing, &cli).status(), 200);
    assert_eq!(
        router.dispatch(&mut Refusing, &cli).body()["message"],
        json!("ok")
    );

    let http = http_command("GET", "/status");
    assert_eq!(router.dispatch(&mut Refusing, &http).status(), 200);

    // A route that is not the default one is reachable too, which is what the
    // old fake-`HttpRequest` approach could not do.
    let post = decode(r#"{"kind":"cli","argv":["POST","/run","a","b"]}"#)
        .expect("decodes")
        .into_command()
        .expect("folds");
    assert_eq!(post.method(), "POST");
    assert_eq!(post.path(), "/run");
    assert_eq!(
        router.dispatch(&mut Refusing, &post).body()["message"],
        json!("a,b")
    );
}

/// `argv` without a method word is a `GET`, which is the common case.
#[test]
fn a_cli_path_without_a_method_is_a_get() {
    let command = decode(r#"{"kind":"cli","argv":["/status"]}"#)
        .expect("decodes")
        .into_command()
        .expect("folds");
    assert_eq!(command.method(), "GET");
    assert_eq!(command.path(), "/status");
}

/// The leading slash is normalized, so the two transports cannot disagree about
/// the same anchor.
#[test]
fn a_cli_path_gains_its_leading_slash() {
    let command = decode(r#"{"kind":"cli","argv":["status"]}"#)
        .expect("decodes")
        .into_command()
        .expect("folds");
    assert_eq!(command.path(), "/status");
}

/// A CLI call that names no operation is refused. Guessing which route the caller
/// meant is how `plugin call` silently runs the wrong thing.
#[test]
fn a_cli_call_with_no_operation_is_a_400() {
    let reply = decode(r#"{"kind":"cli","argv":[]}"#)
        .expect("decodes")
        .into_command()
        .expect_err("an empty argv names nothing");
    assert_eq!(reply.status(), 400);

    let method_only = decode(r#"{"kind":"cli","argv":["POST"]}"#)
        .expect("decodes")
        .into_command()
        .expect_err("a method with no path names nothing");
    assert_eq!(method_only.status(), 400);
}

/// Extra `argv` items become the input, so one handler can read them the same way
/// it reads an HTTP body.
#[test]
fn cli_arguments_become_the_input() {
    let command = decode(r#"{"kind":"cli","argv":["POST","/run","x","y"]}"#)
        .expect("decodes")
        .into_command()
        .expect("folds");
    assert_eq!(command.args(), ["x", "y"]);
    assert_eq!(command.input(), &json!({"args": ["x", "y"]}));
}

/// A CLI call has no query, and asking for one is `None` rather than a mistake.
#[test]
fn a_cli_command_has_no_query() {
    let command = decode(r#"{"kind":"cli","argv":["/status"]}"#)
        .expect("decodes")
        .into_command()
        .expect("folds");
    assert_eq!(command.query("page"), None);
}

/// `input_as` reports a shape mismatch as a `400`, naming what failed.
#[test]
fn a_wrong_input_shape_is_a_400() {
    #[derive(Debug, serde::Deserialize)]
    struct NeedsJob {
        #[allow(dead_code)]
        job: String,
    }

    let command = decode(r#"{"kind":"cli","argv":["/run"]}"#)
        .expect("decodes")
        .into_command()
        .expect("folds");
    let reply = command.input_as::<NeedsJob>().expect_err("no job was sent");
    assert_eq!(reply.status(), 400);
    assert!(
        reply.body()["error"]
            .as_str()
            .unwrap()
            .contains("wrong shape")
    );
}
