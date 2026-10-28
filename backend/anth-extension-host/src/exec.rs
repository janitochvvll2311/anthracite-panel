//! `exec` capabilities.
//!
//! The guest never supplies a finished argv. It names a capability; the
//! manifest supplies the program and argv template; the host validates the
//! values and runs the result as a non-root user inside a sandbox.
//!
//! `SystemExecRunner` is the real side. `DemoExecRunner` is deterministic and
//! never spawns a process, so demo mode exercises the same contract without
//! touching the host.

use std::collections::BTreeMap;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use anth_extension::manifest::{ArgType, Capability};
use anth_wire::{ErrorKind, WireError};
use serde_json::Value;
use wait_timeout::ChildExt;

/// One completed `exec` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecOutput {
    pub program: String,
    pub stdout: String,
    pub stderr: String,
    pub status: i32,
}

pub trait ExecRunner: Send + Sync {
    fn run(&self, capability: &Capability, args: &Value) -> Result<ExecOutput, WireError>;
}

/// Applies filesystem/network isolation inside the child.
///
/// The implementation runs in `pre_exec`, after `fork` and before `exec`, so it
/// must only call async-signal-safe functions.
pub trait ExecSandbox: Send + Sync {
    /// Checks availability before the parent forks.
    fn probe(&self) -> Result<(), String> {
        Ok(())
    }

    fn apply(&self) -> Result<(), String>;
}

/// Refuses every call because the host has no sandbox implementation.
pub struct UnavailableSandbox;

impl ExecSandbox for UnavailableSandbox {
    fn probe(&self) -> Result<(), String> {
        Err("the exec sandbox is unavailable".to_string())
    }

    fn apply(&self) -> Result<(), String> {
        Err("the exec sandbox is unavailable".to_string())
    }
}

pub struct SystemExecRunner {
    uid: u32,
    gid: u32,
    timeout: Duration,
    max_output: usize,
    sandbox: Arc<dyn ExecSandbox>,
}

impl SystemExecRunner {
    pub fn new(uid: u32, gid: u32, sandbox: Arc<dyn ExecSandbox>) -> Self {
        Self {
            uid,
            gid,
            timeout: Duration::from_secs(10),
            max_output: 64 * 1024,
            sandbox,
        }
    }

    /// The default runner: no sandbox means no execution.
    pub fn unavailable() -> Self {
        Self::new(0, 0, Arc::new(UnavailableSandbox))
    }

    fn run_inner(&self, capability: &Capability, args: &Value) -> Result<ExecOutput, WireError> {
        self.sandbox
            .probe()
            .map_err(|detail| WireError::new(ErrorKind::Forbidden, "exec_unavailable", detail))?;
        let argv = argv(capability, args).map_err(|detail| {
            WireError::new(ErrorKind::Invalid, "invalid_capability_args", detail)
        })?;
        let program = argv.first().cloned().unwrap_or_default();
        let mut command = Command::new(&program);
        command
            .args(&argv[1..])
            .env_clear()
            .env("LC_ALL", "C")
            .env("PATH", "/usr/bin:/bin")
            .current_dir("/tmp")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let uid = self.uid;
        let gid = self.gid;
        let sandbox = Arc::clone(&self.sandbox);
        unsafe {
            command.pre_exec(move || {
                if libc::setgroups(0, std::ptr::null()) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::setgid(gid) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::setresuid(uid, uid, uid) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                sandbox.apply().map_err(std::io::Error::other)?;
                Ok(())
            });
        }

        let mut child = command.spawn().map_err(|error| {
            WireError::new(
                ErrorKind::Internal,
                "exec_spawn_failed",
                format!("spawning `{program}`: {error}"),
            )
        })?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let stdout_reader = read_capped(stdout, self.max_output);
        let stderr_reader = read_capped(stderr, self.max_output);

        let status = match child.wait_timeout(self.timeout) {
            Ok(Some(status)) => status,
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(WireError::new(
                    ErrorKind::TooLarge,
                    "exec_timeout",
                    format!("`{program}` exceeded {} seconds", self.timeout.as_secs()),
                ));
            }
            Err(error) => {
                return Err(WireError::internal(format!(
                    "waiting for `{program}`: {error}"
                )));
            }
        };

        let stdout = stdout_reader.join().unwrap_or_default();
        let stderr = stderr_reader.join().unwrap_or_default();
        if stdout.len() > self.max_output || stderr.len() > self.max_output {
            return Err(WireError::too_large(format!(
                "`{program}` produced more than {} bytes",
                self.max_output
            )));
        }
        Ok(ExecOutput {
            program,
            stdout: String::from_utf8_lossy(&stdout).trim_end().to_string(),
            stderr: String::from_utf8_lossy(&stderr).trim_end().to_string(),
            status: status.code().unwrap_or(-1),
        })
    }
}

impl ExecRunner for SystemExecRunner {
    fn run(&self, capability: &Capability, args: &Value) -> Result<ExecOutput, WireError> {
        self.run_inner(capability, args)
    }
}

/// Deterministic `exec` output for the demo build.
pub struct DemoExecRunner {
    replies: BTreeMap<String, ExecOutput>,
}

impl DemoExecRunner {
    pub fn new() -> Self {
        let reply = |program: &str, stdout: &str| ExecOutput {
            program: program.to_string(),
            stdout: stdout.to_string(),
            stderr: String::new(),
            status: 0,
        };
        Self {
            replies: BTreeMap::from([
                ("host.hostname".to_string(), reply("hostname", "demo-host")),
                (
                    "host.uname".to_string(),
                    reply("uname", "Linux anthracite 6.1.0-demo x86_64"),
                ),
                ("host.id".to_string(), reply("id", "1000")),
            ]),
        }
    }
}

impl Default for DemoExecRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl ExecRunner for DemoExecRunner {
    fn run(&self, capability: &Capability, _args: &Value) -> Result<ExecOutput, WireError> {
        self.replies.get(&capability.id).cloned().ok_or_else(|| {
            WireError::new(
                ErrorKind::NotFound,
                "exec_not_simulated",
                format!("the demo has no simulated output for `{}`", capability.id),
            )
        })
    }
}

fn read_capped(
    stream: Option<impl Read + Send + 'static>,
    _limit: usize,
) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buffer = Vec::new();
        if let Some(mut stream) = stream {
            let _ = stream.read_to_end(&mut buffer);
        }
        buffer
    })
}

fn argv(capability: &Capability, args: &Value) -> Result<Vec<String>, String> {
    let map = match args {
        Value::Null => serde_json::Map::new(),
        Value::Object(map) => map.clone(),
        _ => return Err("the arguments must be an object".to_string()),
    };
    let mut values = std::collections::HashMap::new();
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
                validate(name, text)?;
                text.to_string()
            }
            ArgType::Integer => value
                .as_i64()
                .or_else(|| value.as_u64().and_then(|n| i64::try_from(n).ok()))
                .ok_or_else(|| format!("`{name}` must be an integer"))?
                .to_string(),
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
                validate(name, text)?;
                if text.len() > 15
                    || text.contains('/')
                    || text.contains("..")
                    || text.starts_with('.')
                    || text.chars().any(char::is_whitespace)
                {
                    return Err(format!("`{name}` is not a plain interface name"));
                }
                text.to_string()
            }
        };
        values.insert(name.as_str(), rendered);
    }
    let mut argv = vec![capability.program.clone()];
    for template in &capability.args {
        let Some(open) = template.find('{') else {
            argv.push(template.clone());
            continue;
        };
        let after = &template[open + 1..];
        let close = after
            .find('}')
            .ok_or_else(|| format!("the argv template `{template}` has an unclosed placeholder"))?;
        let name = &after[..close];
        if !capability
            .accept
            .iter()
            .any(|accepted| accepted.name == name)
        {
            return Err(format!(
                "the capability `{}` uses `{{{name}}}` but does not accept it",
                capability.id
            ));
        }
        let value = values
            .get(name)
            .ok_or_else(|| format!("the capability `{}` requires `{name}`", capability.id))?;
        argv.push(format!(
            "{}{}{}",
            &template[..open],
            value,
            &after[close + 1..]
        ));
    }
    Ok(argv)
}

fn validate(name: &str, value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("`{name}` must not be empty"));
    }
    if value.len() > 512 {
        return Err(format!("`{name}` is longer than 512 bytes"));
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
    use anth_extension::manifest::ExtensionManifest;
    use serde_json::json;

    fn capability() -> Capability {
        let manifest = ExtensionManifest::parse(
            r#"
manifest_version = 2
[extension]
id = "org.anth.demo"
version = "0.4.0"
name = "Demo"
abi = 3
[runtime]
kind = "wasm"
[wasm]
path = "extension.wasm"
sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
[[capability]]
id = "host.hostname"
program = "hostname"
"#,
        )
        .expect("manifest");
        manifest.capability.into_iter().next().expect("capability")
    }

    #[test]
    fn the_demo_runner_is_deterministic() {
        let runner = DemoExecRunner::new();
        let first = runner.run(&capability(), &json!({})).expect("run");
        let second = runner.run(&capability(), &json!({})).expect("run");
        assert_eq!(first, second);
        assert_eq!(first.stdout, "demo-host");
    }

    #[test]
    fn an_unknown_demo_capability_is_refused() {
        let runner = DemoExecRunner::new();
        let mut capability = capability();
        capability.id = "host.unknown".to_string();
        assert!(runner.run(&capability, &json!({})).is_err());
    }

    #[test]
    fn the_system_runner_refuses_without_a_sandbox() {
        let runner = SystemExecRunner::unavailable();
        let error = runner
            .run(&capability(), &json!({}))
            .expect_err("unavailable sandbox");
        assert!(error.message.contains("sandbox"), "{}", error.message);
    }
}
