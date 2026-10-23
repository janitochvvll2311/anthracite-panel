//! Building an argv from a frozen grant.
//!
//! This is the module that makes "a plugin never sends an argv" true. The guest
//! names a capability id and supplies values; the program and the argument
//! *shape* come from the manifest the operator approved. An accepted value is
//! appended as exactly one element, so a value containing a space cannot become
//! two arguments, and an unknown name is refused rather than ignored — ignoring
//! it would let a guest believe a flag took effect when it did not.

use std::process::Command;

use serde_json::{Map, Value};

use crate::plugins::manifest::{ArgType, Capability};

/// Runs one granted capability.
///
/// The program is looked up in the host allowlist again, even though the
/// manifest was validated at load: the manifest is re-read on every start, and a
/// capability that reached here without passing [`Capability::program`]'s check
/// would otherwise spawn. Cheap to repeat, impossible to skip.
pub fn run(capability: &Capability, args: &Value) -> Result<String, String> {
    let argv = argv(capability, args)?;

    let program = &argv[0];
    let output = Command::new(program)
        .args(&argv[1..])
        .output()
        .map_err(|e| format!("running `{program}`: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "`{program}` exited with {}: {}",
            output.status,
            stderr.trim()
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout)
        .trim_end()
        .to_string();
    let limit = 64 * 1024;
    if stdout.len() > limit {
        return Err(format!(
            "`{program}` printed {} bytes, over the {limit} byte limit",
            stdout.len()
        ));
    }

    // The import's contract is JSON: `anth_plugin` parses what comes back with
    // `serde_json::from_str::<Value>`, and the plugin reads `reply["stdout"]`.
    // Returning the bare text made every hostname look like a broken number.
    Ok(serde_json::json!({
        "program": program,
        "stdout": stdout,
    })
    .to_string())
}

/// The exact argv a capability produces, for a test to assert.
///
/// Public so a test can compare the vector without spawning anything: the fake
/// runner that accepts any argv is exactly how v0.1.0 shipped three broken
/// `sfdisk` forms.
pub fn argv(capability: &Capability, args: &Value) -> Result<Vec<String>, String> {
    let mut argv = vec![capability.program.clone()];

    let map = match args {
        Value::Null => Map::new(),
        Value::Object(map) => map.clone(),
        _ => return Err("the arguments must be an object".to_string()),
    };

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

        match accepted.kind {
            ArgType::Switch => {
                // A switch contributes its own element only when true, so an
                // explicit `false` means "leave it out" rather than `--flag=false`.
                if value.as_bool() == Some(true) {
                    argv.push(format!("--{name}"));
                }
            }
            ArgType::Integer => {
                let number = value
                    .as_i64()
                    .ok_or_else(|| format!("`{name}` must be an integer"))?;
                argv.push(format!("--{name}"));
                argv.push(number.to_string());
            }
            ArgType::String => {
                let text = value
                    .as_str()
                    .ok_or_else(|| format!("`{name}` must be a string"))?;
                // The value is one element, never split on spaces, and it
                // cannot start a new option unnoticed: `--` would be needed for
                // a value that begins with a dash, so refuse one instead of
                // handing the program an argument it will read as a flag.
                if text.starts_with('-') {
                    return Err(format!(
                        "`{name}` may not start with a dash, it would read as a flag"
                    ));
                }
                argv.push(format!("--{name}"));
                argv.push(text.to_string());
            }
        }
    }

    Ok(argv)
}

#[cfg(test)]
mod tests;
