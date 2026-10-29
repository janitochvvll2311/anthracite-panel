//! Building an argv from a frozen grant.
//!
//! This is the module that makes "an extension never sends an argv" true. The guest
//! names a capability and supplies values; the program and the argument
//! *template* come from the manifest the operator approved. Each substituted
//! value is validated by the host and becomes exactly one argv element.

use std::path::PathBuf;

use crate::app_host::exec::{ExecRunner, ExecSpec};
use serde_json::{Map, Value};

use crate::apps::manifest::{ArgType, Capability, MAX_ARG_LEN, PROGRAM_ALLOWLIST};

/// Runs one granted capability through the selected runner.
pub fn run(
    runner: &dyn ExecRunner,
    capability: &Capability,
    args: &Value,
) -> Result<String, String> {
    let argv = argv(capability, args)?;
    let program = &argv[0];
    if !PROGRAM_ALLOWLIST.contains(&program.as_str()) {
        return Err(format!("`{program}` is not on the host allowlist"));
    }
    let spec = ExecSpec {
        id: capability.id.clone(),
        program: PathBuf::from(program),
        args: argv[1..].to_vec(),
    };
    let output = runner.run(&spec).map_err(|error| error.message)?;
    if output.status != 0 {
        return Err(format!(
            "`{}` exited with {}: {}",
            output.program,
            output.status,
            output.stderr.trim()
        ));
    }
    Ok(serde_json::json!({
        "program": output.program,
        "stdout": output.stdout,
        "stderr": output.stderr,
        "status": output.status,
    })
    .to_string())
}

/// The exact argv a capability produces, for a test to assert.
pub fn argv(capability: &Capability, args: &Value) -> Result<Vec<String>, String> {
    let map = match args {
        Value::Null => Map::new(),
        Value::Object(map) => map.clone(),
        _ => return Err("the arguments must be an object".to_string()),
    };

    let mut values: std::collections::HashMap<&str, String> = std::collections::HashMap::new();
    for (name, value) in &map {
        let accepted = capability
            .accept
            .iter()
            .find(|accepted| &accepted.name == name)
            .ok_or_else(|| {
                format!(
                    "the capability `{}` does not accept `{name}`",
                    capability.id
                )
            })?;
        let rendered = match accepted.kind {
            ArgType::String => {
                let text = value
                    .as_str()
                    .ok_or_else(|| format!("`{name}` must be a string"))?;
                validate_value(name, text)?;
                text.to_string()
            }
            ArgType::Integer => {
                let number = value
                    .as_i64()
                    .or_else(|| value.as_u64().and_then(|n| i64::try_from(n).ok()))
                    .ok_or_else(|| format!("`{name}` must be an integer"))?;
                number.to_string()
            }
            ArgType::Switch => {
                if value.as_bool() == Some(true) {
                    format!("--{name}")
                } else {
                    continue;
                }
            }
            ArgType::Ifname => {
                let text = value
                    .as_str()
                    .ok_or_else(|| format!("`{name}` must be a string"))?;
                validate_ifname(name, text)?;
                text.to_string()
            }
        };
        values.insert(name.as_str(), rendered);
    }

    let mut argv = vec![capability.program.clone()];
    for template in &capability.args {
        let mut replaced = String::new();
        let mut rest = template.as_str();
        let mut only_switch = true;
        while let Some(open) = rest.find('{') {
            replaced.push_str(&rest[..open]);
            let after = &rest[open + 1..];
            let close = after.find('}').ok_or_else(|| {
                format!("the argv template `{template}` has an unclosed placeholder")
            })?;
            let name = &after[..close];
            let accepted = capability
                .accept
                .iter()
                .find(|accepted| accepted.name == name)
                .ok_or_else(|| {
                    format!(
                        "the capability `{}` uses `{{{name}}}` but does not accept it",
                        capability.id
                    )
                })?;
            let value = values.get(name).ok_or_else(|| {
                if accepted.kind == ArgType::Switch {
                    return format!("the switch `{name}` has no value");
                }
                format!("the capability `{}` requires `{name}`", capability.id)
            });
            match value {
                Ok(value) => {
                    only_switch = false;
                    replaced.push_str(value);
                }
                Err(_) if accepted.kind == ArgType::Switch => {
                    // An absent switch is false: the whole element disappears.
                }
                Err(error) => return Err(error),
            }
            rest = &after[close + 1..];
        }
        replaced.push_str(rest);
        if only_switch && replaced.is_empty() {
            continue;
        }
        argv.push(replaced);
    }
    Ok(argv)
}

fn validate_ifname(name: &str, value: &str) -> Result<(), String> {
    validate_value(name, value)?;
    if value.len() > 15
        || value.contains('/')
        || value.contains("..")
        || value.starts_with('.')
        || value.chars().any(char::is_whitespace)
    {
        return Err(format!("`{name}` is not a plain interface name"));
    }
    Ok(())
}

fn validate_value(name: &str, value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("`{name}` must not be empty"));
    }
    if value.len() > MAX_ARG_LEN {
        return Err(format!("`{name}` is longer than {MAX_ARG_LEN} bytes"));
    }
    if value.chars().any(char::is_control) {
        return Err(format!("`{name}` contains a control character"));
    }
    if value.starts_with('-') {
        return Err(format!("`{name}` must not start with `-`"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apps::manifest::AppManifest as Manifest;

    fn manifest() -> Manifest {
        Manifest::parse(&format!(
            r#"
manifest_version = 2
[app]
id = "org.anth.hello"
version = "0.3.0"
name = "Hello"
[runtime]
kind = "wasm"

[wasm]
path = "wasm/plugin.wasm"
abi = 3
sha256 = "{}"

[[capability]]
id = "host.named"
program = "hostname"
args = ["--alias", "{{name}}"]
accept = [{{ name = "name", type = "string" }}]

[[capability]]
id = "host.port"
program = "hostname"
args = ["--port", "{{port}}"]
accept = [{{ name = "port", type = "integer" }}]

[[capability]]
id = "host.flag"
program = "hostname"
args = ["{{verbose}}"]
accept = [{{ name = "verbose", type = "switch" }}]
"#,
            "a".repeat(64)
        ))
        .expect("manifest")
    }

    fn capability(id: &str) -> Capability {
        manifest()
            .capability
            .into_iter()
            .find(|capability| capability.id == id)
            .expect("capability")
    }

    #[test]
    fn placeholders_render_one_validated_element() {
        let named = argv(
            &capability("host.named"),
            &serde_json::json!({ "name": "Ana Sol" }),
        )
        .expect("argv");
        assert_eq!(named, ["hostname", "--alias", "Ana Sol"]);

        let port =
            argv(&capability("host.port"), &serde_json::json!({ "port": 22 })).expect("argv");
        assert_eq!(port, ["hostname", "--port", "22"]);
    }

    #[test]
    fn a_false_switch_disappears_and_a_true_one_is_its_element() {
        assert_eq!(
            argv(
                &capability("host.flag"),
                &serde_json::json!({ "verbose": true })
            )
            .expect("true"),
            ["hostname", "--verbose"]
        );
        assert_eq!(
            argv(
                &capability("host.flag"),
                &serde_json::json!({ "verbose": false })
            )
            .expect("false"),
            ["hostname"]
        );
    }

    #[test]
    fn an_interface_name_is_validated_before_wg_sees_it() {
        let manifest = Manifest::parse(&format!(
            r#"
manifest_version = 2
[app]
id = "org.anth.wireguard"
version = "0.3.0"
name = "WireGuard"
[runtime]
kind = "wasm"
[wasm]
path = "wasm/plugin.wasm"
abi = 3
sha256 = "{}"
[[capability]]
id = "wg.show"
program = "wg"
args = ["show", "{{interface}}"]
accept = [{{ name = "interface", type = "ifname" }}]
"#,
            "a".repeat(64)
        ))
        .expect("manifest");
        let capability = manifest.capability.into_iter().next().expect("capability");
        assert_eq!(
            argv(&capability, &serde_json::json!({ "interface": "wg0" })).expect("valid"),
            ["wg", "show", "wg0"]
        );
        for bad in ["wg/0", "wg 0", "..", "this-name-is-too-long"] {
            assert!(
                argv(&capability, &serde_json::json!({ "interface": bad })).is_err(),
                "{bad} must be refused"
            );
        }
    }

    #[test]
    fn unknown_missing_or_dangerous_values_are_refused() {
        assert!(
            argv(
                &capability("host.named"),
                &serde_json::json!({ "other": "x" })
            )
            .is_err()
        );
        assert!(argv(&capability("host.port"), &serde_json::json!({})).is_err());
        assert!(
            argv(
                &capability("host.named"),
                &serde_json::json!({ "name": "-rf" })
            )
            .is_err()
        );
        assert!(
            argv(
                &capability("host.named"),
                &serde_json::json!({ "name": "a\nb" })
            )
            .is_err()
        );
    }
}

#[cfg(test)]
mod runner_tests {
    use super::*;
    use crate::app_host::exec::DemoExecRunner;

    #[test]
    fn a_granted_capability_goes_through_the_runner() {
        let capability = Capability {
            id: "host.hostname".to_string(),
            program: "hostname".to_string(),
            args: Vec::new(),
            accept: Vec::new(),
        };
        let output = run(&DemoExecRunner::new(), &capability, &serde_json::json!({}))
            .expect("runner output");
        assert!(output.contains("demo-host"), "{output}");
    }
}
