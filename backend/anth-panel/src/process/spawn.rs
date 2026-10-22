use super::prelude::*;

/// Upper bound on arguments accepted in one spawn request.
pub const MAX_SPAWN_ARGS: usize = 64;
/// Upper bound on one argument, in bytes.
pub const MAX_SPAWN_ARG_BYTES: usize = 4096;

/// Starts a program from the allowlist.
///
/// The checks are ordered from cheapest to most expensive and every refusal
/// names its reason, because a caller that only sees "denied" cannot tell a
/// typo from a policy. `spawn_enabled` and `allow` come from the daemon's
/// configuration; an empty allowlist denies everything, so turning the switch
/// on without listing a program opens nothing.
pub fn spawn(
    spawn_enabled: bool,
    allow: &[String],
    program: &str,
    args: &[String],
    cwd: Option<&str>,
) -> Result<ProcessSpawned, ProcessSpawnError> {
    if !spawn_enabled {
        return Err(spawn_error(
            ProcessSpawnErrorKind::Disabled,
            "process spawning is disabled".to_string(),
        ));
    }
    if args.len() > MAX_SPAWN_ARGS {
        return Err(spawn_error(
            ProcessSpawnErrorKind::Failed,
            format!("at most {MAX_SPAWN_ARGS} arguments are accepted"),
        ));
    }
    if args.iter().any(|arg| arg.len() > MAX_SPAWN_ARG_BYTES) {
        return Err(spawn_error(
            ProcessSpawnErrorKind::Failed,
            format!("an argument exceeds {MAX_SPAWN_ARG_BYTES} bytes"),
        ));
    }

    let requested = Path::new(program);
    if !requested.is_absolute() {
        return Err(spawn_error(
            ProcessSpawnErrorKind::NotAbsolute,
            format!("`{program}` is not an absolute path"),
        ));
    }

    // Canonicalize before the allowlist comparison: `/bin/echo` and
    // `/usr/bin/echo` are the same file on a merged-usr host, and comparing
    // the text would reject a program the operator did list under its other
    // name. The canonical path is also what gets executed, so a symlink cannot
    // point at a program outside the allowlist.
    let resolved = fs::canonicalize(requested).map_err(|error| {
        spawn_error(
            ProcessSpawnErrorKind::NotFound,
            format!("`{program}` cannot be resolved: {error}"),
        )
    })?;
    let allow_contains = allow.iter().any(|entry| {
        Path::new(entry)
            .is_absolute()
            .then(|| fs::canonicalize(entry).ok())
            .flatten()
            .is_some_and(|allowed| allowed == resolved)
    });
    if !allow_contains {
        return Err(spawn_error(
            ProcessSpawnErrorKind::NotAllowed,
            format!("`{program}` is not in the spawn allowlist"),
        ));
    }

    let metadata = fs::metadata(&resolved).map_err(|error| {
        spawn_error(
            ProcessSpawnErrorKind::NotFound,
            format!("`{}` cannot be read: {error}", resolved.display()),
        )
    })?;
    if !metadata.is_file() {
        return Err(spawn_error(
            ProcessSpawnErrorKind::NotExecutable,
            format!("`{}` is not a regular file", resolved.display()),
        ));
    }
    if metadata.permissions().mode() & 0o111 == 0 {
        return Err(spawn_error(
            ProcessSpawnErrorKind::NotExecutable,
            format!("`{}` is not executable", resolved.display()),
        ));
    }

    if let Some(directory) = cwd {
        let path = Path::new(directory);
        if !path.is_absolute() || !path.is_dir() {
            return Err(spawn_error(
                ProcessSpawnErrorKind::NotFound,
                format!("`{directory}` is not an absolute, existing directory"),
            ));
        }
    }

    let mut command = Command::new(&resolved);
    command.args(args);
    if let Some(directory) = cwd {
        command.current_dir(directory);
    }
    // Detached: the child must not hold the daemon's pipes open, or a chatty
    // program would fill a buffer nobody reads and then block forever.
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    // `setsid()` puts the child in its own session and process group, so a
    // signal aimed at the daemon's group does not take an intentionally
    // long-running program down with it. Without it the child inherits the
    // daemon's group and only survives a plain daemon-only exit. A cgroup-wide
    // stop still kills the child: `setsid` changes the session, not the cgroup.
    //
    // SAFETY: `setsid` is async-signal-safe and touches no memory in this
    // process; it is the documented shape for `pre_exec`. A failure is fatal to
    // the exec, which is why it is checked rather than ignored.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }

    let mut child = command.spawn().map_err(|error| {
        let kind = if error.kind() == io::ErrorKind::PermissionDenied {
            ProcessSpawnErrorKind::PermissionDenied
        } else {
            ProcessSpawnErrorKind::Failed
        };
        spawn_error(kind, format!("failed to start the program: {error}"))
    })?;
    let pid = child.id();

    // The daemon outlives its children, so somebody has to reap them or every
    // short-lived spawn leaves a zombie until the daemon restarts. A thread per
    // child is the smallest thing that does that without touching the global
    // `SIGCHLD` disposition, which would affect the PTY sessions too.
    std::thread::spawn(move || {
        let _ = child.wait();
    });

    Ok(ProcessSpawned {
        pid,
        program: resolved.to_string_lossy().into_owned(),
        args: args.to_vec(),
    })
}

fn spawn_error(kind: ProcessSpawnErrorKind, detail: String) -> ProcessSpawnError {
    ProcessSpawnError { kind, detail }
}
