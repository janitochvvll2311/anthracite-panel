//! WireGuard logic that can run on the host in tests.
//!
//! This cut is read-only: it lists interfaces and shows the human `wg show`
//! output. Interface creation, config editing and peer changes are the next
//! cut, because they need host-owned jobs and secret handling that this plugin
//! deliberately does not pretend to have.

use anth_plugin::{Executor, Response};

pub const INTERFACES_CAPABILITY: &str = "wg.interfaces";
pub const SHOW_CAPABILITY: &str = "wg.show";

pub fn interfaces(executor: &mut dyn Executor) -> Response {
    match executor.exec_bare(INTERFACES_CAPABILITY) {
        Ok(reply) => {
            let stdout = reply
                .get("stdout")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            let names: Vec<&str> = stdout
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .collect();
            Response::ok(serde_json::json!({ "interfaces": names }))
        }
        Err(detail) => Response::failed(detail),
    }
}

pub fn show(executor: &mut dyn Executor, interface: &str) -> Response {
    if interface.is_empty() {
        return Response::bad_request("name one interface, for example `wg0`");
    }
    let args = serde_json::json!({ "interface": interface });
    match executor.exec(SHOW_CAPABILITY, &args) {
        Ok(reply) => {
            let output = reply
                .get("stdout")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            Response::ok(serde_json::json!({
                "interface": interface,
                "output": output,
            }))
        }
        Err(detail) => Response::failed(detail),
    }
}

/// The interface a direct/anchor/CLI value names.
pub fn interface_of(value: &serde_json::Value) -> Option<&str> {
    value
        .get("interface")
        .and_then(|value| value.as_str())
        .or_else(|| {
            value
                .get("args")
                .and_then(|value| value.as_array())
                .and_then(|args| args.first())
                .and_then(|value| value.as_str())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use serde_json::Value;

    struct Fake {
        replies: BTreeMap<String, Result<Value, String>>,
        asked: Vec<(String, Value)>,
    }

    impl Executor for Fake {
        fn exec(&mut self, capability: &str, args: &Value) -> Result<Value, String> {
            self.asked.push((capability.to_string(), args.clone()));
            self.replies
                .get(capability)
                .cloned()
                .unwrap_or_else(|| Err(format!("no reply for {capability}")))
        }

        fn log_at(&mut self, _level: anth_plugin::LogLevel, _message: &str) {}
    }

    fn fake() -> Fake {
        Fake {
            replies: BTreeMap::from([
                (
                    INTERFACES_CAPABILITY.to_string(),
                    Ok(serde_json::json!({ "stdout": "wg0\nwg1\n" })),
                ),
                (
                    SHOW_CAPABILITY.to_string(),
                    Ok(serde_json::json!({ "stdout": "interface: wg0\n" })),
                ),
            ]),
            asked: Vec::new(),
        }
    }

    #[test]
    fn interfaces_split_the_host_output() {
        let reply = interfaces(&mut fake());
        assert_eq!(reply.status, anth_plugin::Status::Ok);
        assert_eq!(reply.value["interfaces"], serde_json::json!(["wg0", "wg1"]));
    }

    #[test]
    fn show_passes_one_validated_interface_name() {
        let mut executor = fake();
        let reply = show(&mut executor, "wg0");
        assert_eq!(reply.status, anth_plugin::Status::Ok);
        assert_eq!(reply.value["output"], "interface: wg0\n");
        assert_eq!(executor.asked[0].0, SHOW_CAPABILITY);
        assert_eq!(executor.asked[0].1["interface"], "wg0");
    }

    #[test]
    fn a_missing_name_is_refused_without_a_capability() {
        let mut executor = fake();
        assert_eq!(show(&mut executor, "").status, anth_plugin::Status::BadRequest);
        assert!(executor.asked.is_empty());
    }

    #[test]
    fn a_refused_capability_is_a_500() {
        let mut executor = Fake {
            replies: BTreeMap::from([(
                SHOW_CAPABILITY.to_string(),
                Err("`wg` is not installed".to_string()),
            )]),
            asked: Vec::new(),
        };
        let reply = show(&mut executor, "wg0");
        assert_eq!(reply.status, anth_plugin::Status::Failed);
        assert_eq!(reply.value["error"], "`wg` is not installed");
    }

    #[test]
    fn an_interface_can_come_from_an_http_body_or_cli_args() {
        assert_eq!(
            interface_of(&serde_json::json!({ "interface": "wg0" })),
            Some("wg0")
        );
        assert_eq!(
            interface_of(&serde_json::json!({ "args": ["wg1"] })),
            Some("wg1")
        );
        assert_eq!(interface_of(&serde_json::json!({})), None);
    }
}
