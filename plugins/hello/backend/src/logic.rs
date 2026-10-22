//! The panel-facing logic of the hello plugin.
//!
//! Kept apart from the wasm entry point so it can be tested on the host: this
//! module touches no import, no memory and no allocator, and reaches the daemon
//! only through [`anth_plugin::Executor`].

use anth_plugin::{Executor, Method, Response};

/// The anchor that reads the host's name; must match `[[api]]` in `plugin.toml`.
pub const GREETING_PATH: &str = "/greeting";

/// A second anchor that runs no capability, so `plugin call` has something to
/// reach that a browser also reaches. Two routes are what make "the CLI extends
/// to what the plugin supports" a claim a test can check.
pub const STATUS_PATH: &str = "/status";

/// Every route this plugin declares.
///
/// A test compares this with `plugin.toml`, because v0.1.0 shipped a manifest
/// that named a route no module served and nothing failed until mount time.
pub const ROUTES: &[(Method, &str)] = &[(Method::Get, GREETING_PATH), (Method::Get, STATUS_PATH)];

/// The capability it runs; must match `[[capability]]` in `plugin.toml`.
const HOSTNAME: &str = "host.hostname";

/// The capability name, exposed so a test can compare it with the manifest.
pub fn hostname_capability() -> &'static str {
    HOSTNAME
}

/// `GET /greeting` reads the host's name and answers it.
///
/// The capability is declared in `plugin.toml` with a frozen argv; the guest
/// supplies no program and no argument, only the request to run it. `exec_bare`
/// passes `{}` rather than `null`, which the host's argument map would refuse.
pub fn greeting(executor: &mut dyn Executor) -> Response {
    match executor.exec_bare(HOSTNAME) {
        Ok(reply) => {
            let hostname = reply
                .get("stdout")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown")
                .trim();
            Response::message(format!("Hola desde {hostname}"))
        }
        // A refused capability is the operator's problem to see, not a `200`
        // with an empty name.
        Err(detail) => Response::failed(detail),
    }
}

/// `GET /status` names the plugin and whatever the caller sent.
///
/// No capability: it answers from its own manifest, so it works on a host that
/// granted nothing. Echoing `value` is what shows a CLI call arriving with its
/// flags and options intact, and an HTTP call arriving with its query.
pub fn status(value: &serde_json::Value) -> Response {
    Response::ok(serde_json::json!({
        "plugin": "org.anth.hello",
        "state": "ready",
        "received": value,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::BTreeMap;

    use anth_plugin::{CliRequest, HttpRequest, Plugin};
    use serde_json::Value;

    /// Canned replies keyed by capability, so a test asserts what the guest
    /// asked for and what it made of the answer.
    struct Fake {
        replies: BTreeMap<String, Result<Value, String>>,
        asked: Vec<(String, Value)>,
    }

    impl Fake {
        fn answering(stdout: &str) -> Self {
            let mut replies = BTreeMap::new();
            replies.insert(
                HOSTNAME.to_string(),
                Ok(serde_json::json!({"ok": true, "stdout": stdout, "stderr": ""})),
            );
            Self {
                replies,
                asked: Vec::new(),
            }
        }
    }

    impl Executor for Fake {
        fn exec(&mut self, capability: &str, args: &Value) -> Result<Value, String> {
            self.asked.push((capability.to_string(), args.clone()));
            self.replies
                .get(capability)
                .cloned()
                .unwrap_or_else(|| Err(format!("no reply for {capability}")))
        }

        fn log(&mut self, _message: &str) {}
    }

    /// Runs one anchor through the plugin's real HTTP mapper.
    fn get(path: &str) -> anth_plugin::Response {
        let request = HttpRequest {
            method: "GET".to_string(),
            path: path.to_string(),
            query: Default::default(),
            body: None,
        };
        let mapped = crate::Hello::from_http(request).expect("the mapper knows this anchor");
        crate::Hello.handle(&mut Fake::answering("turing\n"), mapped)
    }

    #[test]
    fn the_greeting_names_the_host_the_capability_returned() {
        let mut fake = Fake::answering("turing\n");

        let reply = greeting(&mut fake);

        assert_eq!(reply.status, anth_plugin::Status::Ok);
        assert_eq!(
            reply.value["message"],
            serde_json::json!("Hola desde turing"),
            "the trailing newline must be trimmed"
        );
        assert_eq!(fake.asked.len(), 1);
        assert_eq!(fake.asked[0].0, HOSTNAME);
    }

    /// The host decodes arguments into a map, so `null` is refused while `{}`
    /// works. This pins that the plugin asks with an object.
    #[test]
    fn the_capability_is_asked_with_an_object_not_null() {
        let mut fake = Fake::answering("box");
        let _ = greeting(&mut fake);

        let args = &fake.asked[0].1;
        assert!(args.is_object(), "arguments must be an object, got {args}");
    }

    #[test]
    fn a_refused_capability_is_a_500_carrying_the_host_detail() {
        let mut fake = Fake {
            replies: BTreeMap::from([(
                HOSTNAME.to_string(),
                Err("`hostname` is not installed".to_string()),
            )]),
            asked: Vec::new(),
        };

        let reply = greeting(&mut fake);

        assert_eq!(reply.status, anth_plugin::Status::Failed);
        assert_eq!(
            reply.value["error"],
            serde_json::json!("`hostname` is not installed")
        );
    }

    #[test]
    fn a_reply_without_stdout_still_answers() {
        let mut fake = Fake {
            replies: BTreeMap::from([(HOSTNAME.to_string(), Ok(serde_json::json!({"ok": true})))]),
            asked: Vec::new(),
        };

        let reply = greeting(&mut fake);

        assert_eq!(reply.status, anth_plugin::Status::Ok);
        assert_eq!(
            reply.value["message"],
            serde_json::json!("Hola desde unknown")
        );
    }

    /// The second route runs no capability, so it answers even with nothing
    /// granted: a plugin is not only its capabilities.
    #[test]
    fn the_status_route_needs_no_capability() {
        let reply = get(STATUS_PATH);

        assert_eq!(reply.status, anth_plugin::Status::Ok);
        assert_eq!(reply.value["state"], serde_json::json!("ready"));
    }

    /// Every declared route maps and answers, which is the property the manifest
    /// test relies on when it compares `ROUTES` with `plugin.toml`.
    #[test]
    fn every_declared_route_answers() {
        for (_, path) in ROUTES {
            let reply = get(path);
            assert_eq!(
                reply.status,
                anth_plugin::Status::Ok,
                "no mapper reaches {path}"
            );
        }
    }

    /// The CLI alias reaches the same action the browser does.
    #[test]
    fn a_cli_subcommand_reaches_the_greeting() {
        let request = CliRequest {
            argv: vec!["greet".to_string()],
        };
        let mapped = crate::Hello::from_cli(request).expect("`greet` is declared");
        assert_eq!(mapped.action, crate::GREETING);

        let reply = crate::Hello.handle(&mut Fake::answering("turing\n"), mapped);
        assert_eq!(
            reply.value["message"],
            serde_json::json!("Hola desde turing")
        );
    }

    /// The status action echoes what the transport carried, so a CLI call shows
    /// its alias and its words without the plugin inventing them.
    #[test]
    fn the_cli_status_reports_the_words_it_received() {
        let request = CliRequest {
            argv: vec!["status".to_string(), "--verbose".to_string()],
        };
        let mapped = crate::Hello::from_cli(request).expect("`status` is declared");

        let reply = status(&mapped.value);

        assert_eq!(
            reply.value["received"]["command"],
            serde_json::json!("status")
        );
        assert_eq!(
            reply.value["received"]["flags"]["verbose"],
            serde_json::json!(true)
        );
    }

    /// A subcommand outside the table is refused with the ones that are in it.
    #[test]
    fn an_unknown_cli_subcommand_lists_the_declared_ones() {
        let request = CliRequest {
            argv: vec!["frobnicate".to_string()],
        };

        let refused = crate::Hello::from_cli(request).unwrap_err();

        assert_eq!(refused.status, anth_plugin::Status::BadRequest);
        assert!(
            refused.value["error"].as_str().unwrap().contains("greet"),
            "the message must list the declared subcommands: {}",
            refused.value
        );
    }
}
