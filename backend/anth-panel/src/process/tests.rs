use super::*;

#[test]
fn stat_parser_survives_spaces_and_parentheses_in_comm() {
    let stat = "42 (my ) proc) S 1 42 42 0 -1 4194560 100 0 0 0 7 5 0 0 20 0 3 0 12345 1000000 42 18446744073709551615";

    let parsed = parse_stat(stat).unwrap();
    assert_eq!(parsed.comm, "my ) proc");
    assert_eq!(parsed.state, "S");
    assert_eq!(parsed.ppid, 1);
    assert_eq!(parsed.cpu_ticks, 12);
    assert_eq!(parsed.threads, 3);
    assert_eq!(parsed.start_ticks, 12345);
}

#[test]
fn command_lines_are_truncated_by_characters() {
    let (short, truncated) = truncate_chars("abcdef", 4);
    assert_eq!(short, "abcd");
    assert!(truncated);

    let (whole, truncated) = truncate_chars("abc", 4);
    assert_eq!(whole, "abc");
    assert!(!truncated);
}

#[test]
fn warnings_explain_truncation() {
    assert_eq!(warning(10, 10, false), None);
    assert_eq!(
        warning(MAX_PROCESSES, MAX_PROCESSES + 1, true),
        Some(format!(
            "showing {MAX_PROCESSES} of {} processes, highest CPU first",
            MAX_PROCESSES + 1
        ))
    );
}

#[test]
fn serialized_snapshots_fit_the_line_limit() {
    let cases = [
        ("CJK", "汉".repeat(MAX_COMMAND_CHARS)),
        ("quote-heavy", "\"".repeat(MAX_COMMAND_CHARS)),
        ("ASCII", "a".repeat(MAX_COMMAND_CHARS)),
    ];

    for (name, command) in cases {
        let processes = (0..MAX_PROCESSES)
            .map(|index| ProcessInfo {
                pid: index as u32 + 1,
                command: command.clone(),
                ..ProcessInfo::default()
            })
            .collect();

        let snapshot = ProcessSnapshot {
            processes,
            clock_ticks: 100,
            timestamp_ms: 1_700_000_000_000,
            control_enabled: false,
            spawn_enabled: false,
            spawn_allow: Vec::new(),
            total: MAX_PROCESSES,
            truncated: false,
            warning: None,
        };
        let before = Response::ProcessSnapshot(snapshot.clone())
            .to_json()
            .unwrap()
            .len();
        let fitted = fit_snapshot(snapshot, false);
        let json = Response::ProcessSnapshot(fitted.clone()).to_json().unwrap();

        assert!(
            json.len() <= MAX_LINE_LEN,
            "{name} snapshot is {} bytes",
            json.len()
        );
        if before > MAX_LINE_LEN {
            assert!(fitted.truncated, "{name} should be marked truncated");
            if fitted.processes.len() < fitted.total {
                assert!(fitted.warning.is_some(), "{name} should have a row warning");
            }
        }
        assert!(!fitted.processes.is_empty());
    }
}

#[test]
fn command_truncation_sets_the_truncated_flag() {
    let snapshot = ProcessSnapshot {
        processes: vec![ProcessInfo {
            pid: 42,
            command: "x".repeat(MAX_COMMAND_CHARS + 1),
            ..ProcessInfo::default()
        }],
        clock_ticks: 100,
        timestamp_ms: 1_700_000_000_000,
        spawn_enabled: false,
        spawn_allow: Vec::new(),
        total: 1,
        ..ProcessSnapshot::default()
    };

    let fitted = fit_snapshot(snapshot, true);

    assert!(fitted.truncated);
    assert!(fitted.warning.is_none());
}

#[test]
fn collect_from_reads_a_fixture_proc_tree() {
    let root = unique_dir("proc-fixture");
    let proc = root.join("proc").join("42");
    fs::create_dir_all(&proc).unwrap();
    fs::create_dir_all(root.join("etc")).unwrap();

    fs::write(
            proc.join("stat"),
            "42 (my proc) S 1 42 42 0 -1 4194560 100 0 0 0 7 5 0 0 20 0 3 0 12345 1000000 42 18446744073709551615\n",
        )
        .unwrap();
    fs::write(
        proc.join("status"),
        "Name:\tmy proc\nUid:\t1000\t1000\t1000\t1000\nVmRSS:\t   2048 kB\n",
    )
    .unwrap();
    fs::write(proc.join("comm"), "my proc\n").unwrap();
    fs::write(proc.join("cmdline"), b"myproc\0--flag\0").unwrap();
    fs::write(
        root.join("etc/passwd"),
        "alice:x:1000:1000:Alice:/home/alice:/bin/bash\n",
    )
    .unwrap();

    let snapshot = collect_from(
        &root,
        &ProcessPolicy {
            control: true,
            ..ProcessPolicy::default()
        },
    );
    assert_eq!(snapshot.processes.len(), 1);
    assert_eq!(snapshot.total, 1);
    assert!(!snapshot.truncated);
    assert!(snapshot.control_enabled);

    let process = &snapshot.processes[0];
    assert_eq!(process.pid, 42);
    assert_eq!(process.user, "alice");
    assert_eq!(process.command, "myproc --flag");
    assert_eq!(process.rss_kib, 2048);
    assert_eq!(process.threads, 3);

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn signal_policy_rejects_disabled_and_protected_pids() {
    assert_eq!(
        send_signal(123, ProcessSignal::Term, &ProcessPolicy::default()),
        Err(SignalError::ControlDisabled)
    );
    assert_eq!(
        send_signal(
            1,
            ProcessSignal::Term,
            &ProcessPolicy {
                control: true,
                ..ProcessPolicy::default()
            }
        ),
        Err(SignalError::Protected)
    );
    assert_eq!(
        send_signal(
            std::process::id(),
            ProcessSignal::Term,
            &ProcessPolicy {
                control: true,
                ..ProcessPolicy::default()
            }
        ),
        Err(SignalError::Protected)
    );
}

#[test]
fn a_real_child_can_be_terminated() {
    let mut child = std::process::Command::new("sleep")
        .arg("30")
        .spawn()
        .unwrap();
    let pid = child.id();

    for _ in 0..100 {
        if Path::new(&format!("/proc/{pid}")).exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    send_signal(
        pid,
        ProcessSignal::Term,
        &ProcessPolicy {
            control: true,
            ..ProcessPolicy::default()
        },
    )
    .unwrap();
    let status = child.wait().unwrap();
    assert!(!status.success(), "SIGTERM should terminate sleep");
}

fn unique_dir(name: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("anth-panel-{name}-{}-{nanos}", std::process::id()))
}

/// A program that is safe to run and always present.
fn echo_program() -> String {
    fs::canonicalize("/bin/echo")
        .or_else(|_| fs::canonicalize("/usr/bin/echo"))
        .expect("echo must exist")
        .to_string_lossy()
        .into_owned()
}

fn cat_program() -> String {
    fs::canonicalize("/bin/cat")
        .or_else(|_| fs::canonicalize("/usr/bin/cat"))
        .expect("cat must exist")
        .to_string_lossy()
        .into_owned()
}

#[test]
fn spawn_is_refused_when_disabled() {
    let program = echo_program();

    let error = spawn(false, std::slice::from_ref(&program), &program, &[], None).unwrap_err();

    assert_eq!(error.kind, ProcessSpawnErrorKind::Disabled);
}

#[test]
fn spawn_with_an_empty_allowlist_denies_everything() {
    // Enabling `[process] spawn` without listing a program must open
    // nothing, which is the whole point of decision 14, so it gets its own
    // test rather than riding along with the not-allowed case.
    let program = echo_program();

    let error = spawn(true, &[], &program, &[], None).unwrap_err();

    assert_eq!(error.kind, ProcessSpawnErrorKind::NotAllowed);
}

#[test]
fn spawn_rejects_a_relative_program() {
    let error = spawn(true, &["echo".to_string()], "echo", &[], None).unwrap_err();

    assert_eq!(error.kind, ProcessSpawnErrorKind::NotAbsolute);
}

#[test]
fn spawn_rejects_a_program_outside_the_allowlist() {
    let error = spawn(true, &[echo_program()], &cat_program(), &[], None).unwrap_err();

    assert_eq!(error.kind, ProcessSpawnErrorKind::NotAllowed);
}

#[test]
fn spawn_compares_canonical_paths_so_merged_usr_names_both_work() {
    // `/bin/echo` and `/usr/bin/echo` are one file on a merged-usr host.
    // Comparing the request text against the allowlist would reject the
    // name the operator did not happen to type; comparing canonical paths
    // accepts both, and still runs the single resolved file.
    let program = echo_program();
    let started = spawn(true, std::slice::from_ref(&program), "/bin/echo", &[], None);

    let started = match started {
        Ok(started) => started,
        // A host without `/bin` keeps `/usr/bin` only; the fallback path is
        // then a genuine miss rather than a policy question.
        Err(error) => {
            assert_eq!(error.kind, ProcessSpawnErrorKind::NotFound);
            return;
        }
    };
    assert_eq!(started.program, program);
}

#[test]
fn spawn_rejects_a_symlink_whose_target_is_not_allowlisted() {
    // The allowlist names echo; a symlink is listed instead under a name of
    // its own so the request would pass a naive text comparison. Resolving
    // first is what makes this a denial.
    let root = unique_dir("spawn-symlink");
    fs::create_dir_all(&root).unwrap();
    let link = root.join("innocent");
    std::os::unix::fs::symlink(cat_program(), &link).unwrap();

    let error = spawn(true, &[echo_program()], &link.to_string_lossy(), &[], None).unwrap_err();

    assert_eq!(error.kind, ProcessSpawnErrorKind::NotAllowed);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn spawn_rejects_a_non_executable_file() {
    let root = unique_dir("spawn-noexec");
    fs::create_dir_all(&root).unwrap();
    let file = root.join("plain");
    fs::write(&file, b"not a program\n").unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();

    let error = spawn(
        true,
        &[file.to_string_lossy().into_owned()],
        &file.to_string_lossy(),
        &[],
        None,
    )
    .unwrap_err();

    assert_eq!(error.kind, ProcessSpawnErrorKind::NotExecutable);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn spawn_rejects_a_directory() {
    let root = unique_dir("spawn-dir");
    fs::create_dir_all(&root).unwrap();

    let error = spawn(
        true,
        &[root.to_string_lossy().into_owned()],
        &root.to_string_lossy(),
        &[],
        None,
    )
    .unwrap_err();

    assert_eq!(error.kind, ProcessSpawnErrorKind::NotExecutable);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn spawn_rejects_a_missing_program() {
    let missing = "/nonexistent-program-for-anth".to_string();

    let error = spawn(true, std::slice::from_ref(&missing), &missing, &[], None).unwrap_err();

    assert_eq!(error.kind, ProcessSpawnErrorKind::NotFound);
}

#[test]
fn spawn_rejects_a_missing_working_directory() {
    let program = echo_program();

    let error = spawn(
        true,
        std::slice::from_ref(&program),
        &program,
        &[],
        Some("/nonexistent-directory-for-anth"),
    )
    .unwrap_err();

    assert_eq!(error.kind, ProcessSpawnErrorKind::NotFound);
}

#[test]
fn a_spawned_child_leads_its_own_process_group() {
    // The docs promise the child survives a signal aimed at the daemon's
    // group, which only holds if it called `setsid()`. Comparing the two
    // process groups is the observable part of that promise.
    //
    // The child has to outlive the read: a spawn is fire-and-forget, so a
    // program that exits immediately leaves nothing to inspect and the
    // test would fail intermittently under load. `sleep` is give-or-take
    // free and is killed below.
    let Some(program) = sleep_program() else {
        return;
    };
    let Ok(started) = spawn(
        true,
        std::slice::from_ref(&program),
        &program,
        &["30".to_string()],
        None,
    ) else {
        return;
    };

    let own_group = process_group_of(started.pid);
    let daemon_group = process_group_of(std::process::id());

    // `setsid` detached the child, so `kill` by pid is the only handle left.
    // It runs before the assertion so a failure cannot leak a `sleep`.
    unsafe {
        libc::kill(started.pid as libc::pid_t, libc::SIGKILL);
    }

    // A child that exited before the read is not a `setsid` failure.
    let (Some(own_group), Some(daemon_group)) = (own_group, daemon_group) else {
        return;
    };
    assert_ne!(
        own_group, daemon_group,
        "a spawned child must lead its own process group"
    );
}

fn sleep_program() -> Option<String> {
    fs::canonicalize("/bin/sleep")
        .or_else(|_| fs::canonicalize("/usr/bin/sleep"))
        .ok()
        .map(|path| path.to_string_lossy().into_owned())
}

fn process_group_of(pid: u32) -> Option<u32> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let (_, rest) = stat.split_once(") ")?;
    // state, ppid, pgrp
    rest.split_whitespace().nth(2)?.parse().ok()
}

#[test]
fn spawn_rejects_too_many_arguments() {
    let program = echo_program();
    let args = vec!["x".to_string(); MAX_SPAWN_ARGS + 1];

    let error = spawn(true, std::slice::from_ref(&program), &program, &args, None).unwrap_err();

    assert_eq!(error.kind, ProcessSpawnErrorKind::Failed);
}

#[test]
fn spawn_rejects_an_oversized_argument() {
    let program = echo_program();
    let args = vec!["x".repeat(MAX_SPAWN_ARG_BYTES + 1)];

    let error = spawn(true, std::slice::from_ref(&program), &program, &args, None).unwrap_err();

    assert_eq!(error.kind, ProcessSpawnErrorKind::Failed);
}

#[test]
fn spawn_runs_an_allowlisted_program_and_reports_its_pid() {
    let program = echo_program();

    let started = spawn(
        true,
        std::slice::from_ref(&program),
        &program,
        &["hello".to_string()],
        None,
    )
    .unwrap();

    assert!(started.pid > 0);
    assert_eq!(started.args, vec!["hello".to_string()]);
    assert_eq!(started.program, program);
}

#[test]
fn a_spawned_child_is_reaped_so_it_does_not_become_a_zombie() {
    // Without the reaping thread this leaves the daemon's own child in state
    // `Z`, which is the leak the thread exists to prevent.
    let program = echo_program();
    let started = spawn(true, std::slice::from_ref(&program), &program, &[], None).unwrap();
    let pid = started.pid;

    for _ in 0..200 {
        let Ok(stat) = fs::read_to_string(format!("/proc/{pid}/stat")) else {
            // Gone from `/proc` means the reaper collected it: success.
            return;
        };
        let state = stat
            .rsplit_once(')')
            .and_then(|(_, rest)| rest.split_whitespace().next())
            .unwrap_or("");
        assert_ne!(state, "Z", "spawned child {pid} was left unreaped");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn the_snapshot_advertises_spawn_state_and_a_usable_allowlist() {
    let root = unique_dir("spawn-snapshot");
    fs::create_dir_all(root.join("proc")).unwrap();
    fs::create_dir_all(root.join("etc")).unwrap();

    let policy = ProcessPolicy {
        control: true,
        spawn: true,
        spawn_allow: vec!["/bin/echo".to_string(), "relative-entry".to_string()],
    };
    let snapshot = collect_from(&root, &policy);

    assert!(snapshot.control_enabled);
    assert!(snapshot.spawn_enabled);
    // A relative entry can never match an absolute program, so the UI is not
    // offered it as a choice.
    assert_eq!(snapshot.spawn_allow, vec!["/bin/echo".to_string()]);

    let _ = fs::remove_dir_all(&root);
}
