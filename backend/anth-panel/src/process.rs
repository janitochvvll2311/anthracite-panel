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
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anth_proto::{MAX_LINE_LEN, ProcessInfo, ProcessSignal, ProcessSnapshot, Response};

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

/// Reads the live process table.
pub fn collect(control_enabled: bool) -> ProcessSnapshot {
    collect_from("/", control_enabled)
}

/// Reads a process table from an alternative filesystem root.
pub fn collect_from(root: impl AsRef<Path>, control_enabled: bool) -> ProcessSnapshot {
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
        control_enabled,
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
    control_enabled: bool,
) -> Result<(), SignalError> {
    if !control_enabled {
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

        let snapshot = collect_from(&root, true);
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
            send_signal(123, ProcessSignal::Term, false),
            Err(SignalError::ControlDisabled)
        );
        assert_eq!(
            send_signal(1, ProcessSignal::Term, true),
            Err(SignalError::Protected)
        );
        assert_eq!(
            send_signal(std::process::id(), ProcessSignal::Term, true),
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

        send_signal(pid, ProcessSignal::Term, true).unwrap();
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
}
