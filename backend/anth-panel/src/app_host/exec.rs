//! `exec` capabilities.
//!
//! The guest never supplies a finished argv. The extension host names a
//! capability, validates the manifest and renders one [`ExecSpec`]. This module
//! runs that spec as a non-root user inside a sandbox.
//!
//! `SystemExecRunner` is the real side. `DemoExecRunner` is deterministic and
//! never spawns a process.

use std::collections::BTreeMap;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use anth_wire::{ErrorKind, WireError};
use wait_timeout::ChildExt;

/// One validated command to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecSpec {
    pub id: String,
    pub program: PathBuf,
    pub args: Vec<String>,
}

/// One completed `exec` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecOutput {
    pub program: String,
    pub stdout: String,
    pub stderr: String,
    pub status: i32,
}

pub trait ExecRunner: Send + Sync + std::fmt::Debug {
    fn run(&self, spec: &ExecSpec) -> Result<ExecOutput, WireError>;
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

    fn run_inner(&self, spec: &ExecSpec) -> Result<ExecOutput, WireError> {
        self.sandbox
            .probe()
            .map_err(|detail| WireError::new(ErrorKind::Forbidden, "exec_unavailable", detail))?;
        let program = spec.program.to_string_lossy().into_owned();
        let mut command = Command::new(&spec.program);
        command
            .args(&spec.args)
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
        let stdout_reader = read_capped(stdout);
        let stderr_reader = read_capped(stderr);

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

impl std::fmt::Debug for SystemExecRunner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SystemExecRunner")
            .field("uid", &self.uid)
            .field("gid", &self.gid)
            .field("timeout", &self.timeout)
            .field("max_output", &self.max_output)
            .finish_non_exhaustive()
    }
}

impl ExecRunner for SystemExecRunner {
    fn run(&self, spec: &ExecSpec) -> Result<ExecOutput, WireError> {
        self.run_inner(spec)
    }
}

/// Deterministic `exec` output for the demo build.
#[derive(Debug)]
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
    fn run(&self, spec: &ExecSpec) -> Result<ExecOutput, WireError> {
        self.replies.get(&spec.id).cloned().ok_or_else(|| {
            WireError::new(
                ErrorKind::NotFound,
                "exec_not_simulated",
                format!("the demo has no simulated output for `{}`", spec.id),
            )
        })
    }
}

fn read_capped(stream: Option<impl Read + Send + 'static>) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buffer = Vec::new();
        if let Some(mut stream) = stream {
            let _ = stream.read_to_end(&mut buffer);
        }
        buffer
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(id: &str) -> ExecSpec {
        ExecSpec {
            id: id.to_string(),
            program: PathBuf::from("hostname"),
            args: Vec::new(),
        }
    }

    #[test]
    fn the_demo_runner_is_deterministic() {
        let runner = DemoExecRunner::new();
        let first = runner.run(&spec("host.hostname")).expect("run");
        let second = runner.run(&spec("host.hostname")).expect("run");
        assert_eq!(first, second);
        assert_eq!(first.stdout, "demo-host");
    }

    #[test]
    fn an_unknown_demo_capability_is_refused() {
        let runner = DemoExecRunner::new();
        assert!(runner.run(&spec("host.unknown")).is_err());
    }

    #[test]
    fn the_system_runner_refuses_without_a_sandbox() {
        let runner = SystemExecRunner::unavailable();
        let error = runner.run(&spec("host.hostname")).expect_err("unavailable");
        assert!(error.message.contains("sandbox"), "{}", error.message);
    }
}
