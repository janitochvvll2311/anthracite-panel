//! Linux process inspection and signals.
//!
//! Everything here works through `/proc` and the standard library, with `libc`
//! only for `sysconf`, `getuid` and `kill`. `collect_from` takes a filesystem
//! root so tests can exercise the parsing against fixtures without touching the
//! host's process table.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use anth_proto::{
    MAX_LINE_LEN, ProcessInfo, ProcessSignal, ProcessSnapshot, ProcessSpawnError,
    ProcessSpawnErrorKind, ProcessSpawned, Response,
};

/// Maximum rows returned in one snapshot. The protocol is newline-delimited and
/// one line is capped at 1 MiB, so the serialized snapshot is also fitted to
/// that budget below.
pub const MAX_PROCESSES: usize = 2000;
/// Maximum command-line length in a snapshot, in characters.
pub const MAX_COMMAND_CHARS: usize = 256;
/// Commands are shortened down to this many characters before rows are dropped
/// to satisfy the protocol line limit.
const MIN_COMMAND_CHARS: usize = 8;
const CLOCK_TICKS_FALLBACK: u64 = 100;

#[derive(Debug, PartialEq, Eq)]
pub enum SignalError {
    ControlDisabled,
    Protected,
    NotOwned,
    NotFound,
    PermissionDenied,
    Failed(String),
}

impl SignalError {
    pub fn detail(&self) -> String {
        match self {
            Self::ControlDisabled => "process control is disabled".to_string(),
            Self::Protected => "refusing to signal pid 1 or the daemon's own process".to_string(),
            Self::NotOwned => "the process is not owned by the daemon user".to_string(),
            Self::NotFound => "the process does not exist".to_string(),
            Self::PermissionDenied => "permission denied while signalling the process".to_string(),
            Self::Failed(detail) => detail.clone(),
        }
    }
}

/// The daemon's process policy, resolved from `[process]` once per request.
///
/// One struct instead of three arguments so a new switch cannot be forgotten at
/// one of the call sites, and so the snapshot and the gate read the same values.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProcessPolicy {
    /// Whether signals are accepted.
    pub control: bool,
    /// Whether spawning is accepted.
    pub spawn: bool,
    /// Absolute programs a spawn may start. Empty denies everything.
    pub spawn_allow: Vec<String>,
}

impl ProcessPolicy {
    pub fn new(config: &crate::config::ProcessConfig) -> Self {
        Self {
            control: config.control,
            spawn: config.spawn,
            spawn_allow: config.spawn_allow.clone(),
        }
    }
}

/// Reads the live process table.
pub fn collect(policy: &ProcessPolicy) -> ProcessSnapshot {
    collect_from("/", policy)
}

/// Reads a process table from an alternative filesystem root.
pub fn collect_from(root: impl AsRef<Path>, policy: &ProcessPolicy) -> ProcessSnapshot {
    let root = root.as_ref();
    let users = read_users(&root.join("etc/passwd"));
    let (mut processes, command_truncated) = read_processes(&root.join("proc"), &users);
    let total = processes.len();

    // Truncation keeps the busiest processes, not the lowest pids.
    processes.sort_by(|left, right| {
        right
            .cpu_ticks
            .cmp(&left.cpu_ticks)
            .then_with(|| left.pid.cmp(&right.pid))
    });

    let truncated = total > MAX_PROCESSES;
    if truncated {
        processes.truncate(MAX_PROCESSES);
    }

    let snapshot = ProcessSnapshot {
        processes,
        clock_ticks: clock_ticks(),
        timestamp_ms: timestamp_ms(),
        control_enabled: policy.control,
        spawn_enabled: policy.spawn,
        // An allowlist the client cannot see is a form the operator fills in
        // blind, so the snapshot carries it. Entries are filtered the same way
        // `spawn` filters them, so the UI cannot offer what the gate rejects.
        spawn_allow: policy
            .spawn_allow
            .iter()
            .filter(|entry| Path::new(entry).is_absolute())
            .cloned()
            .collect(),
        total,
        truncated,
        warning: None,
    };

    fit_snapshot(snapshot, command_truncated)
}

/// Sends an allowlisted signal after checking the daemon-side policy.
///
/// The policy is not a UI concern: the daemon is the authority even when a
/// client talks to the socket directly.
pub fn send_signal(
    pid: u32,
    signal: ProcessSignal,
    policy: &ProcessPolicy,
) -> Result<(), SignalError> {
    if !policy.control {
        return Err(SignalError::ControlDisabled);
    }
    if pid <= 1 || pid == std::process::id() {
        return Err(SignalError::Protected);
    }

    let target_uid = read_process_uid(pid).ok_or(SignalError::NotFound)?;
    let own_uid = unsafe { libc::getuid() };
    if target_uid != own_uid {
        return Err(SignalError::NotOwned);
    }

    let result = unsafe { libc::kill(pid as i32, raw_signal(signal)) };
    if result == 0 {
        return Ok(());
    }

    let error = io::Error::last_os_error();
    match error.raw_os_error() {
        Some(libc::ESRCH) => Err(SignalError::NotFound),
        Some(libc::EPERM) => Err(SignalError::PermissionDenied),
        _ => Err(SignalError::Failed(format!(
            "failed to signal process {pid}: {error}"
        ))),
    }
}

fn raw_signal(signal: ProcessSignal) -> i32 {
    match signal {
        ProcessSignal::Term => libc::SIGTERM,
        ProcessSignal::Kill => libc::SIGKILL,
        ProcessSignal::Hup => libc::SIGHUP,
        ProcessSignal::Int => libc::SIGINT,
        ProcessSignal::Quit => libc::SIGQUIT,
        ProcessSignal::Stop => libc::SIGSTOP,
        ProcessSignal::Cont => libc::SIGCONT,
    }
}

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

fn fit_snapshot(mut snapshot: ProcessSnapshot, command_truncated: bool) -> ProcessSnapshot {
    let total = snapshot.total;
    let mut command_truncated = command_truncated;
    let mut command_limit = MAX_COMMAND_CHARS;
    let rows_truncated = snapshot.processes.len() < total;

    snapshot.truncated = rows_truncated || command_truncated;
    snapshot.warning = warning(snapshot.processes.len(), total, rows_truncated);
    let mut encoded_len = snapshot_json_len(&snapshot);

    // Commands are the bulk of a snapshot. Shorten them first so the useful
    // process rows survive, then drop the least busy rows only if necessary.
    while encoded_len > MAX_LINE_LEN && command_limit > MIN_COMMAND_CHARS {
        command_limit = (command_limit / 2).max(MIN_COMMAND_CHARS);
        for process in &mut snapshot.processes {
            let (command, truncated) = truncate_chars(&process.command, command_limit);
            process.command = command;
            command_truncated |= truncated;
        }

        snapshot.truncated = rows_truncated || command_truncated;
        snapshot.warning = warning(snapshot.processes.len(), total, rows_truncated);
        encoded_len = snapshot_json_len(&snapshot);
    }

    if encoded_len > MAX_LINE_LEN {
        // Rows are sorted by CPU descending, so the tail is the least busy.
        // Binary-search the largest prefix that still fits the frame.
        let processes = snapshot.processes.clone();
        let mut low = 0;
        let mut high = processes.len();

        while low < high {
            let mid = (low + high).div_ceil(2);
            let mut candidate = snapshot.clone();
            candidate.processes.truncate(mid);
            candidate.truncated = true;
            candidate.warning = warning(mid, total, true);

            if snapshot_json_len(&candidate) <= MAX_LINE_LEN {
                low = mid;
            } else {
                high = mid - 1;
            }
        }

        snapshot.processes.truncate(low);
        snapshot.truncated = true;
        snapshot.warning = warning(low, total, true);
    }

    snapshot
}

fn snapshot_json_len(snapshot: &ProcessSnapshot) -> usize {
    Response::ProcessSnapshot(snapshot.clone())
        .to_json()
        .map(|json| json.len())
        .unwrap_or(usize::MAX)
}

fn warning(shown: usize, total: usize, rows_truncated: bool) -> Option<String> {
    rows_truncated.then(|| format!("showing {shown} of {total} processes, highest CPU first"))
}

fn timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn clock_ticks() -> u64 {
    let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if ticks > 0 {
        ticks as u64
    } else {
        CLOCK_TICKS_FALLBACK
    }
}

fn read_users(path: &Path) -> HashMap<u32, String> {
    fs::read_to_string(path)
        .ok()
        .map(|text| {
            text.lines()
                .filter_map(|line| {
                    let mut fields = line.split(':');
                    let name = fields.next()?;
                    let _password = fields.next()?;
                    let uid = fields.next()?.parse().ok()?;
                    Some((uid, name.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn read_processes(proc_dir: &Path, users: &HashMap<u32, String>) -> (Vec<ProcessInfo>, bool) {
    let Ok(entries) = fs::read_dir(proc_dir) else {
        return (Vec::new(), false);
    };

    let mut processes = Vec::new();
    let mut command_truncated = false;

    for entry in entries.flatten() {
        let Some(pid) = parse_pid(entry.file_name().as_os_str()) else {
            continue;
        };
        let directory = entry.path();

        let Ok(stat) = fs::read_to_string(directory.join("stat")) else {
            continue;
        };
        let Some(stat) = parse_stat(&stat) else {
            continue;
        };
        let status = fs::read_to_string(directory.join("status")).unwrap_or_default();
        let uid = parse_uid(&status).unwrap_or(0);
        let rss_kib = parse_rss(&status);
        let name = fs::read_to_string(directory.join("comm"))
            .ok()
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| stat.comm.clone());
        let command =
            read_cmdline(&directory.join("cmdline")).unwrap_or_else(|| format!("[{name}]"));
        let (command, truncated) = truncate_chars(&command, MAX_COMMAND_CHARS);
        command_truncated |= truncated;

        processes.push(ProcessInfo {
            pid,
            ppid: stat.ppid,
            uid,
            user: users.get(&uid).cloned().unwrap_or_else(|| uid.to_string()),
            name,
            command,
            state: stat.state,
            cpu_ticks: stat.cpu_ticks,
            rss_kib,
            threads: stat.threads,
            start_ticks: stat.start_ticks,
        });
    }

    (processes, command_truncated)
}

fn parse_pid(name: &OsStr) -> Option<u32> {
    name.to_str()?.parse().ok()
}

struct Stat {
    comm: String,
    state: String,
    ppid: u32,
    cpu_ticks: u64,
    threads: u32,
    start_ticks: u64,
}

/// Parses `/proc/<pid>/stat`. The command name is the only field that may contain
/// spaces and parentheses, so the numeric fields are read after the last `)`.
fn parse_stat(text: &str) -> Option<Stat> {
    let open = text.find('(')?;
    let close = text.rfind(')')?;
    if close <= open {
        return None;
    }

    let comm = text.get(open + 1..close)?.to_string();
    let fields: Vec<&str> = text.get(close + 1..)?.split_whitespace().collect();
    if fields.len() < 22 {
        return None;
    }

    let cpu_ticks = fields[11]
        .parse::<u64>()
        .ok()?
        .saturating_add(fields[12].parse::<u64>().ok()?);

    Some(Stat {
        comm,
        state: fields[0].to_string(),
        ppid: fields[1].parse().ok()?,
        cpu_ticks,
        threads: fields[17].parse().ok()?,
        start_ticks: fields[19].parse().ok()?,
    })
}

fn parse_uid(status: &str) -> Option<u32> {
    status.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        if fields.next()? != "Uid:" {
            return None;
        }
        fields.next()?.parse().ok()
    })
}

fn parse_rss(status: &str) -> u64 {
    status
        .lines()
        .find_map(|line| {
            let mut fields = line.split_whitespace();
            if fields.next()? != "VmRSS:" {
                return None;
            }
            fields.next()?.parse().ok()
        })
        .unwrap_or(0)
}

fn read_cmdline(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    let arguments: Vec<String> = bytes
        .split(|byte| *byte == 0)
        .filter(|argument| !argument.is_empty())
        .map(|argument| String::from_utf8_lossy(argument).into_owned())
        .collect();

    if arguments.is_empty() {
        None
    } else {
        Some(arguments.join(" "))
    }
}

fn truncate_chars(text: &str, limit: usize) -> (String, bool) {
    let mut chars = text.chars();
    let shortened: String = chars.by_ref().take(limit).collect();
    (shortened, chars.next().is_some())
}

fn read_process_uid(pid: u32) -> Option<u32> {
    let status = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    parse_uid(&status)
}

#[cfg(test)]
mod tests {
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
}
