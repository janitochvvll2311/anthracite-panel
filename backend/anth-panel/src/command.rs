//! Command execution with optional sudo escalation.
//!
//! Callers pass an explicit argument vector, never a shell string. When sudo is
//! enabled and cannot elevate (no `NOPASSWD` rule, no TTY), the command is
//! retried directly so a root daemon or a file capability still works.

use std::io;
use std::process::Command;

/// Result of one invocation, decoupled from `std::process::Output` so tests can
/// script responses without spawning a process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandError {
    /// The program, or sudo itself, is not installed or not on PATH.
    Unavailable { program: String },
    /// The command exists but could not be spawned.
    Failed { program: String, detail: String },
}

pub trait CommandRunner {
    fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput, CommandError>;
}

/// Runs commands directly, or through `sudo -n` with a direct retry.
pub struct SystemRunner {
    use_sudo: bool,
}

impl SystemRunner {
    pub fn new(use_sudo: bool) -> Self {
        Self { use_sudo }
    }
}

impl CommandRunner for SystemRunner {
    fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput, CommandError> {
        if !self.use_sudo {
            return execute(program, false, args);
        }

        let elevated = execute(program, true, args);
        if should_fallback_from_sudo(&elevated) {
            execute(program, false, args)
        } else {
            elevated
        }
    }
}

pub fn execute(
    program: &str,
    use_sudo: bool,
    args: &[&str],
) -> Result<CommandOutput, CommandError> {
    let output = command(program, use_sudo, args).env("LC_ALL", "C").output();

    match output {
        Ok(output) => Ok(CommandOutput {
            ok: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: join_stderr(&output.stderr),
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let missing = if use_sudo { "sudo" } else { program };
            Err(CommandError::Unavailable {
                program: missing.to_string(),
            })
        }
        Err(error) => Err(CommandError::Failed {
            program: program.to_string(),
            detail: error.to_string(),
        }),
    }
}

fn join_stderr(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("; ")
}

/// If sudo cannot elevate, retry directly so a file capability or a root daemon
/// still works. Genuine failures of the wrapped command do not trigger a retry.
pub fn should_fallback_from_sudo(result: &Result<CommandOutput, CommandError>) -> bool {
    match result {
        Err(CommandError::Unavailable { .. }) => true,
        Ok(output) if !output.ok => {
            let lowercase = output.stderr.to_lowercase();
            lowercase.contains("a password is required")
                || lowercase.contains("a terminal is required")
                || lowercase.contains("no tty present")
                || lowercase.contains("not in the sudoers")
                || lowercase.contains("not allowed to execute")
                || lowercase.contains("command not found")
                || lowercase.contains("askpass")
        }
        _ => false,
    }
}

pub fn command(program: &str, use_sudo: bool, args: &[&str]) -> Command {
    let mut command = if use_sudo {
        let mut command = Command::new("sudo");
        command.arg("-n").arg(program);
        command
    } else {
        Command::new(program)
    };
    command.args(args);
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sudo_prefix_is_added_before_the_program() {
        let direct = command("nft", false, &["list", "table"]);
        assert_eq!(direct.get_program(), "nft");
        assert_eq!(direct.get_args().collect::<Vec<_>>(), vec!["list", "table"]);

        let elevated = command("mount", true, &["/dev/sda1", "/mnt"]);
        assert_eq!(elevated.get_program(), "sudo");
        assert_eq!(
            elevated.get_args().collect::<Vec<_>>(),
            vec!["-n", "mount", "/dev/sda1", "/mnt"]
        );
    }

    fn failure(stderr: &str) -> Result<CommandOutput, CommandError> {
        Ok(CommandOutput {
            ok: false,
            stdout: String::new(),
            stderr: stderr.to_string(),
        })
    }

    #[test]
    fn only_sudo_failures_fall_back_to_direct_execution() {
        assert!(should_fallback_from_sudo(&failure(
            "sudo: a password is required"
        )));
        assert!(should_fallback_from_sudo(&failure(
            "sudo: no tty present and no askpass program specified"
        )));
        assert!(should_fallback_from_sudo(&failure(
            "user is not in the sudoers file"
        )));
        assert!(should_fallback_from_sudo(&Err(CommandError::Unavailable {
            program: "sudo".to_string(),
        })));
        assert!(!should_fallback_from_sudo(&failure("mount: wrong fs type")));
        assert!(!should_fallback_from_sudo(&Ok(CommandOutput {
            ok: true,
            stdout: String::new(),
            stderr: String::new(),
        })));
    }

    #[test]
    fn stderr_lines_are_joined() {
        let output = CommandOutput {
            ok: false,
            stdout: String::new(),
            stderr: join_stderr(b"first line\n\n  second line  \n"),
        };
        assert_eq!(output.stderr, "first line; second line");
    }

    #[test]
    fn a_missing_program_is_unavailable() {
        let error = execute("anth-definitely-not-a-real-binary", false, &[]).unwrap_err();
        assert_eq!(
            error,
            CommandError::Unavailable {
                program: "anth-definitely-not-a-real-binary".to_string(),
            }
        );
    }

    #[test]
    fn a_real_command_reports_success() {
        let output = execute("true", false, &[]).unwrap();
        assert!(output.ok);
    }
}
