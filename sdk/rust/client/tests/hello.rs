use std::sync::{Arc, Mutex};

use anth_client::http::GreetingHttpClient;
use anth_client::{ClientError, Transport, COMMANDS};

type Calls = Arc<Mutex<Vec<(String, String, Option<String>)>>>;

#[derive(Clone)]
struct Fake {
    calls: Calls,
    reply: String,
}

impl Fake {
    fn new(reply: &str) -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            reply: reply.to_string(),
        }
    }

    fn calls(&self) -> Calls {
        Arc::clone(&self.calls)
    }
}

impl Transport for Fake {
    fn request(&self, method: &str, path: &str, body: Option<&str>) -> Result<String, ClientError> {
        self.calls.lock().expect("lock").push((
            method.to_string(),
            path.to_string(),
            body.map(str::to_string),
        ));
        Ok(self.reply.clone())
    }
}

#[test]
fn the_http_client_builds_the_route_and_parses_the_result() {
    let fake = Fake::new("\"hello, Ada\"");
    let calls = fake.calls();
    let client = GreetingHttpClient::new(fake);
    assert_eq!(
        client.greet("Ada".to_string()).expect("greet"),
        "hello, Ada"
    );
    let calls = calls.lock().expect("lock");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "GET");
    assert_eq!(calls[0].1, "/api/apps/org.anthracite.hello/hello?name=Ada");
    assert_eq!(calls[0].2, None);
}

#[test]
fn the_cli_client_parses_flags_and_calls_the_same_route() {
    let fake = Fake::new("\"hello, Ada\"");
    let calls = fake.calls();
    let output = anth_client::cli::run(fake, "greet", &["--name".to_string(), "Ada".to_string()])
        .expect("run");
    assert_eq!(output, "\"hello, Ada\"");
    let calls = calls.lock().expect("lock");
    assert_eq!(calls[0].1, "/api/apps/org.anthracite.hello/hello?name=Ada");
}

#[test]
fn the_cli_client_accepts_inline_flags() {
    let fake = Fake::new("\"hello, Ada\"");
    let calls = fake.calls();
    let output = anth_client::cli::run(fake, "greet", &["--name=Ada".to_string()]).expect("run");
    assert_eq!(output, "\"hello, Ada\"");
    assert_eq!(
        calls.lock().expect("lock")[0].1,
        "/api/apps/org.anthracite.hello/hello?name=Ada"
    );
}

#[test]
fn the_cli_client_reports_missing_flags() {
    let error =
        anth_client::cli::run(Fake::new("\"unused\""), "greet", &[]).expect_err("missing flag");
    assert!(error.to_string().contains("missing --name"));
}

#[test]
fn the_generated_command_table_matches_the_manifest() {
    assert_eq!(COMMANDS.len(), 1);
    let command = &COMMANDS[0];
    assert_eq!(command.verb, "greet");
    assert_eq!(command.method, Some("GET"));
    assert_eq!(command.path, Some("/hello"));
    assert_eq!(command.interface, "anthracite:hello/greeting@1.0.0");
    assert_eq!(command.function, "greet");
    assert_eq!(command.returns, "string");
    assert_eq!(command.params[0].name, "name");
    assert!(!command.params[0].optional);
}
