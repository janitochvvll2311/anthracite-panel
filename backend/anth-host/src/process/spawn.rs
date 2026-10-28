//! Starting one allowlisted program, detached and reaped.
//!
//! This is a fire-and-forget start, not a job queue: stdio is null, there is no
//! supervision and no output. The child calls `setsid()` so it leads its own
//! session, and a thread waits for it so the daemon never accumulates zombies.
//! `SIGCHLD` is left alone because the PTYs rely on the default disposition.

use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

use anth_wire::{ErrorKind, MAX_SPAWN_ARG_BYTES, MAX_SPAWN_ARGS, ProcessStarted, WireError};

/// Starts `program` when the switch is on and the canonical path is listed.
///
/// An empty allowlist denies everything even with the switch on: the list is
/// the authority, so an operator who forgets it gets a refusal, not an open
/// launcher.
pub fn spawn(
    enabled: bool,
    allow: &[String],
    program: &str,
    args: &[String],
    cwd: Option<&str>,
) -> Result<ProcessStarted, WireError> {
    if !enabled {
        return Err(WireError::new(
            ErrorKind::Forbidden,
            "process_spawn_disabled",
            "process spawning is disabled; set `[process] spawn = true`",
        ));
    }
    if args.len() > MAX_SPAWN_ARGS {
        return Err(WireError::new(
            ErrorKind::Invalid,
            "too_many_arguments",
            format!("at most {MAX_SPAWN_ARGS} arguments are accepted"),
        ));
    }
    if args.iter().any(|arg| arg.len() > MAX_SPAWN_ARG_BYTES) {
        return Err(WireError::new(
            ErrorKind::Invalid,
            "argument_too_large",
            format!("an argument exceeds {MAX_SPAWN_ARG_BYTES} bytes"),
        ));
    }

    let requested = Path::new(program);
    if !requested.is_absolute() {
        return Err(WireError::new(
            ErrorKind::Invalid,
            "program_not_absolute",
            format!("`{program}` is not an absolute path"),
        ));
    }

    // Canonicalize before comparing: `/bin/echo` and `/usr/bin/echo` are the
    // same file on a merged-usr host, and the canonical path is what runs, so a
    // symlink cannot point outside the allowlist.
    let resolved = fs::canonicalize(requested).map_err(|error| {
        WireError::not_found(format!("`{program}` cannot be resolved: {error}"))
    })?;
    if !allow
        .iter()
        .any(|entry| resolves_to(entry, program, &resolved))
    {
        return Err(WireError::new(
            ErrorKind::Forbidden,
            "program_not_allowed",
            format!("`{program}` is not in the spawn allowlist"),
        ));
    }

    let metadata = fs::metadata(&resolved)
        .map_err(|error| WireError::not_found(format!("`{program}` cannot be read: {error}")))?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
        return Err(WireError::new(
            ErrorKind::Forbidden,
            "program_not_executable",
            format!("`{program}` is not an executable file"),
        ));
    }

    if let Some(directory) = cwd {
        let path = Path::new(directory);
        if !path.is_absolute() {
            return Err(WireError::new(
                ErrorKind::Invalid,
                "cwd_not_absolute",
                format!("`{directory}` is not an absolute path"),
            ));
        }
        if !path.is_dir() {
            return Err(WireError::new(
                ErrorKind::NotFound,
                "cwd_not_found",
                format!("`{directory}` is not an existing directory"),
            ));
        }
    }

    let mut command = Command::new(&resolved);
    // Keep the requested path as `argv[0]`: busybox applets are symlinks to one
    // binary and pick their applet from argv[0], so running the canonical target
    // would run the wrong program. The binary that actually executes is the one
    // the allowlist matched.
    command.arg0(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(directory) = cwd {
        command.current_dir(directory);
    }

    // SAFETY: `setsid` is async-signal-safe and touches no memory in this
    // process; it is the documented shape for `pre_exec`. A failure is fatal to
    // the exec, so it is checked instead of ignored.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }

    let mut child = command.spawn().map_err(|error| {
        if error.kind() == io::ErrorKind::PermissionDenied {
            WireError::new(
                ErrorKind::Forbidden,
                "spawn_permission",
                format!("permission denied while starting `{program}`"),
            )
        } else {
            WireError::internal(format!("failed to start `{program}`: {error}"))
        }
    })?;
    let pid = child.id();

    // One thread per child waits for it. Without this the daemon keeps a
    // zombie for every program it started.
    std::thread::Builder::new()
        .name(format!("anth-spawn-{pid}"))
        .spawn(move || {
            let _ = child.wait();
        })
        .map_err(|error| WireError::internal(format!("failed to reap `{program}`: {error}")))?;

    Ok(ProcessStarted {
        pid,
        program: program.to_string(),
        args: args.to_vec(),
    })
}

/// Whether one allowlist entry names the file that is about to run.
///
/// Two equal canonical paths are not enough: every busybox applet is a symlink
/// to one binary, so `/bin/true` and `/bin/sleep` share a target and listing one
/// would allow the other (including `/bin/sh`). Requiring the same file name
/// keeps applets distinct while still accepting the merged-usr alias
/// `/bin/echo` ≡ `/usr/bin/echo`.
fn resolves_to(entry: &str, requested: &str, resolved: &Path) -> bool {
    let entry = Path::new(entry);
    if !entry.is_absolute() {
        return false;
    }
    let same_name = entry.file_name() == Path::new(requested).file_name();
    same_name
        && fs::canonicalize(entry)
            .map(|allowed| allowed == resolved)
            .unwrap_or(false)
}
