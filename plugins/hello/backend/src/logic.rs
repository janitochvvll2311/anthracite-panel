//! The panel-facing logic of the hello plugin.
//!
//! Kept apart from the wasm entry point so it can be tested on the host: this
//! module touches no import, no memory and no allocator, and reaches the daemon
//! only through [`anth_plugin::Executor`].

use anth_plugin::{Command, Executor, Method, Reply};

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
pub fn greeting(executor: &mut dyn Executor, _command: &Command) -> Reply {
    match executor.exec_bare(HOSTNAME) {
        Ok(reply) => {
            let hostname = reply
                .get("stdout")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown")
                .trim();
            Reply::message(format!("Hola desde {hostname}"))
        }
        // A refused capability is the operator's problem to see, not a `200`
        // with an empty name.
        Err(detail) => Reply::failed(detail),
    }
}

/// `GET /status` names the plugin and the arguments the caller sent.
///
/// No capability: it answers from its own manifest, so it works on a host that
/// granted nothing. The echo of `args()` is what shows a CLI call arriving with
/// its positional arguments intact.
pub fn status(_executor: &mut dyn Executor, command: &Command) -> Reply {
    Reply::ok(serde_json::json!({
        "plugin": "org.anth.hello",
        "state": "ready",
        "args": command.args(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::BTreeMap;

    use anth_plugin::Router;
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

    fn get(path: &str) -> Command {
        Command::new("GET", path, BTreeMap::new(), serde_json::json!({}))
    }

    fn router() -> Router {
        Router::new()
            .get(GREETING_PATH, greeting)
            .get(STATUS_PATH, status)
    }

    #[test]
    fn the_greeting_names_the_host_the_capability_returned() {
        let mut fake = Fake::answering("turing\n");

        let reply = greeting(&mut fake, &get(GREETING_PATH));

        assert_eq!(reply.status(), 200);
        assert_eq!(
            reply.body()["message"],
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
        let _ = greeting(&mut fake, &get(GREETING_PATH));

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

        let reply = greeting(&mut fake, &get(GREETING_PATH));

        assert_eq!(reply.status(), 500);
        assert_eq!(
            reply.body()["error"],
            serde_json::json!("`hostname` is not installed")
        );
    }

    #[test]
    fn a_reply_without_stdout_still_answers() {
        let mut fake = Fake {
            replies: BTreeMap::from([(HOSTNAME.to_string(), Ok(serde_json::json!({"ok": true})))]),
            asked: Vec::new(),
        };

        let reply = greeting(&mut fake, &get(GREETING_PATH));

        assert_eq!(reply.status(), 200);
        assert_eq!(
            reply.body()["message"],
            serde_json::json!("Hola desde unknown")
        );
    }

    /// The second route runs no capability, so it answers even with nothing
    /// granted: a plugin is not only its capabilities.
    #[test]
    fn the_status_route_needs_no_capability() {
        let reply = router().dispatch(&mut Fake::answering(""), &get(STATUS_PATH));

        assert_eq!(reply.status(), 200);
        assert_eq!(reply.body()["state"], serde_json::json!("ready"));
    }

    /// Every declared route answers, which is the property the CLI relies on.
    #[test]
    fn every_declared_route_answers() {
        let mut fake = Fake::answering("box");
        for (_, path) in ROUTES {
            let reply = router().dispatch(&mut fake, &get(path));
            assert_eq!(reply.status(), 200, "no handler answered {path}");
        }
    }
}
