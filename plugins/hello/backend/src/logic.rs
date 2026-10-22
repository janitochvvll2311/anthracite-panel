//! The panel-facing logic of the hello plugin.
//!
//! Kept apart from the wasm shim so it can be tested on the host: this module
//! touches no import, no memory and no allocator, and reaches the daemon only
//! through [`Executor`].

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use serde::Deserialize;
use serde_json::{Value, json};

/// The host's `anth_exec`, as a seam.
///
/// The wasm build implements it over the imported function; host tests
/// implement it over a table of canned replies. A refusal is an `Err` carrying
/// the daemon's detail, never a panic: a plugin that cannot reach its program
/// still has to answer the browser.
pub trait Executor {
    fn exec(&mut self, capability: &str, args: &Value) -> Result<Value, String>;
    fn log(&mut self, message: &str);
}

/// One request envelope, as `plugins::api::anchor` forwards it.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Request {
    Http(HttpRequest),
    Cli { argv: Vec<String> },
}

#[derive(Debug, Deserialize)]
pub struct HttpRequest {
    pub method: String,
    pub path: String,
    #[serde(default)]
    pub body: Option<Value>,
}

/// Parses an envelope and answers it.
///
/// A malformed envelope is a `400` the browser can display, not a trap: a guest
/// that aborts leaves the panel with a dead request and no explanation.
pub fn handle<E: Executor>(executor: &mut E, request: &str) -> String {
    match serde_json::from_str::<Request>(request) {
        Ok(Request::Http(request)) => route(executor, &request),
        Ok(Request::Cli { argv }) => cli(executor, &argv),
        Err(error) => envelope(
            400,
            json!({"error": format!("the request is not an envelope: {error}")}),
        ),
    }
}

pub fn envelope(status: u16, body: Value) -> String {
    json!({"status": status, "body": body}).to_string()
}

fn failure(status: u16, error: String) -> String {
    envelope(status, json!({"error": error}))
}

/// `GET /greeting` reads the host's name and answers it.
///
/// The capability is declared in `plugin.toml` with a frozen argv; the guest
/// supplies no program and no argument, only the request to run it.
pub fn route<E: Executor>(executor: &mut E, request: &HttpRequest) -> String {
    if request.method != "GET" || request.path != "/greeting" {
        return failure(
            404,
            format!(
                "no route for {} {}",
                request.method.to_uppercase(),
                request.path
            ),
        );
    }

    match executor.exec("host.hostname", &Value::Null) {
        Ok(reply) => {
            let hostname = reply
                .get("stdout")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .trim()
                .to_string();
            envelope(200, json!({"message": format!("Hola desde {hostname}")}))
        }
        Err(detail) => failure(500, detail),
    }
}

/// `plugin call <id> <request>` reaches the same routes.
pub fn cli<E: Executor>(executor: &mut E, argv: &[String]) -> String {
    let path = argv.first().map(String::as_str).unwrap_or("/greeting");
    let request = HttpRequest {
        method: "GET".to_string(),
        path: path.to_string(),
        body: None,
    };
    route(executor, &request)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::collections::BTreeMap;

    /// Canned replies keyed by capability, so a test asserts what the guest
    /// asked for and what it made of the answer.
    struct Fake {
        replies: BTreeMap<String, Result<Value, String>>,
        asked: Vec<String>,
    }

    impl Fake {
        fn answering(stdout: &str) -> Self {
            let mut replies = BTreeMap::new();
            replies.insert(
                "host.hostname".to_string(),
                Ok(json!({"status": 0, "stdout": stdout})),
            );
            Self {
                replies,
                asked: Vec::new(),
            }
        }
    }

    impl Executor for Fake {
        fn exec(&mut self, capability: &str, _args: &Value) -> Result<Value, String> {
            self.asked.push(capability.to_string());
            self.replies
                .get(capability)
                .cloned()
                .unwrap_or_else(|| Err(format!("no reply for {capability}")))
        }

        fn log(&mut self, _message: &str) {}
    }

    fn body(response: &str) -> Value {
        let parsed: Value = serde_json::from_str(response).expect("the answer must be JSON");
        parsed["body"].clone()
    }

    #[test]
    fn the_greeting_names_the_host_the_capability_returned() {
        let mut fake = Fake::answering("turing\n");
        let request = r#"{"kind":"http","method":"GET","path":"/greeting"}"#;

        let response = handle(&mut fake, request);

        assert!(response.contains(r#""status":200"#), "{response}");
        assert_eq!(
            body(&response)["message"],
            json!("Hola desde turing"),
            "the trailing newline must be trimmed"
        );
        assert_eq!(fake.asked, ["host.hostname"]);
    }

    #[test]
    fn a_refused_capability_is_a_500_with_the_host_detail() {
        let mut fake = Fake::answering("");
        fake.replies.insert(
            "host.hostname".to_string(),
            Err("hostname is unreadable".into()),
        );

        let response = handle(
            &mut fake,
            r#"{"kind":"http","method":"GET","path":"/greeting"}"#,
        );

        assert!(response.contains(r#""status":500"#), "{response}");
        assert_eq!(body(&response)["error"], json!("hostname is unreadable"));
    }

    #[test]
    fn an_unknown_route_is_a_404_naming_it() {
        let mut fake = Fake::answering("turing");
        let response = handle(
            &mut fake,
            r#"{"kind":"http","method":"POST","path":"/nope"}"#,
        );

        assert!(response.contains(r#""status":404"#), "{response}");
        assert_eq!(body(&response)["error"], json!("no route for POST /nope"));
        assert!(fake.asked.is_empty(), "a miss must not reach the daemon");
    }

    #[test]
    fn a_malformed_envelope_is_a_400_not_a_trap() {
        let mut fake = Fake::answering("turing");
        let response = handle(&mut fake, "{not json");

        assert!(response.contains(r#""status":400"#), "{response}");
        assert!(
            body(&response)["error"]
                .as_str()
                .expect("an error string")
                .starts_with("the request is not an envelope"),
            "{response}"
        );
    }

    #[test]
    fn the_cli_anchor_reaches_the_same_route() {
        let mut fake = Fake::answering("turing");
        let cli = handle(&mut fake, r#"{"kind":"cli","argv":["/greeting"]}"#);

        assert!(cli.contains(r#""status":200"#), "{cli}");
        assert_eq!(body(&cli)["message"], json!("Hola desde turing"));
    }
}
