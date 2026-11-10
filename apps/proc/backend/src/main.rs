//! Native Processes service: `/proc`, allowlisted signals and detached spawn.
//!
//! This is the real half of the `proc` app. It reads the host process table,
//! computes CPU deltas between two snapshots, refuses signals without the
//! matching `start-ticks` (so a recycled pid cannot be hit) and only spawns a
//! small allowlist of detached helpers.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anth_contract::Error;
use anth_service::Handler;
use serde_json::{json, Value};

const PROC: &str = "anthracite:proc/proc@1.1.0";
const MAX_PAGE: u32 = 100_000;
const DEFAULT_PAGE: u32 = 50;
const MAX_ARGV: usize = 4;
const MAX_ARGUMENT: usize = 128;
const SPAWN_ALLOWLIST: &[&str] = &["sleep", "echo", "true"];
const SIGNALS: &[(&str, i32)] = &[
    ("HUP", libc::SIGHUP),
    ("INT", libc::SIGINT),
    ("TERM", libc::SIGTERM),
    ("KILL", libc::SIGKILL),
    ("STOP", libc::SIGSTOP),
    ("CONT", libc::SIGCONT),
];

struct Service {
    sample: Mutex<Sample>,
    /// Pagination asks for several pages of the same table; one scan per page
    /// would read every `/proc/<pid>` entry eight times. A short cache makes a
    /// tree refresh one snapshot instead.
    cache: Mutex<Option<Cached>>,
}

struct Cached {
    at: Instant,
    stats: Vec<Stat>,
    percents: BTreeMap<u32, f64>,
    users: BTreeMap<u32, String>,
}

#[derive(Default)]
struct Sample {
    at: Option<Instant>,
    total: u64,
    processes: BTreeMap<u32, (u64, u64)>,
}

#[derive(Debug, Clone)]
struct Stat {
    pid: u32,
    ppid: u32,
    name: String,
    state: char,
    ticks: u64,
    start_ticks: u64,
    memory: u64,
    uid: u32,
}

impl Default for Service {
    fn default() -> Self {
        Self::new()
    }
}

impl Service {
    fn new() -> Self {
        Self {
            sample: Mutex::new(Sample::default()),
            cache: Mutex::new(None),
        }
    }

    fn list(&self, input: &Value) -> Result<Value, Error> {
        let cursor = match optional_text(input, "cursor") {
            Some(cursor) => cursor
                .parse::<u32>()
                .map_err(|_| Error::invalid("`cursor` is not a page cursor"))?,
            None => 0,
        };
        let limit = number(input, "limit")
            .unwrap_or(u64::from(DEFAULT_PAGE))
            .clamp(1, u64::from(MAX_PAGE)) as usize;

        let (stats, percents, users) = self.snapshot();

        let remaining: Vec<&Stat> = stats
            .iter()
            .filter(|process| process.pid > cursor)
            .collect();
        let end = remaining.len().min(limit);
        let page = &remaining[..end];
        let done = end == remaining.len();
        let next = page
            .last()
            .map(|process| process.pid.to_string())
            .filter(|_| !done);

        Ok(json!({
            "processes": page
                .iter()
                .map(|process| json!({
                    "pid": process.pid,
                    "ppid": process.ppid,
                    "name": process.name,
                    "user": user_name(&users, process.uid),
                    "state": state_of(process.state),
                    "cpu-percent": percents.get(&process.pid).copied().unwrap_or(0.0),
                    "memory-bytes": process.memory,
                    "start-ticks": process.start_ticks,
                }))
                .collect::<Vec<_>>(),
            "cursor": next,
            "done": done,
        }))
    }

    fn snapshot(&self) -> (Vec<Stat>, BTreeMap<u32, f64>, BTreeMap<u32, String>) {
        const TTL: Duration = Duration::from_secs(2);
        let mut cache = self.cache.lock().expect("the snapshot cache is poisoned");
        if let Some(cached) = cache.as_ref() {
            if cached.at.elapsed() < TTL {
                return (
                    cached.stats.clone(),
                    cached.percents.clone(),
                    cached.users.clone(),
                );
            }
        }
        let mut stats = snapshot();
        stats.sort_by_key(|process| process.pid);
        let percents = self.sample(&stats);
        let users = users();
        *cache = Some(Cached {
            at: Instant::now(),
            stats: stats.clone(),
            percents: percents.clone(),
            users: users.clone(),
        });
        (stats, percents, users)
    }

    fn sample(&self, stats: &[Stat]) -> BTreeMap<u32, f64> {
        let now = Instant::now();
        let total = total_ticks();
        let mut sample = self.sample.lock().expect("the sample mutex is poisoned");
        let mut percents = BTreeMap::new();
        if let Some(at) = sample.at {
            let elapsed = now.duration_since(at).as_secs_f64();
            let total_delta = total.saturating_sub(sample.total);
            if elapsed > 0.0 && total_delta > 0 {
                let cores = std::thread::available_parallelism()
                    .map(|value| value.get() as f64)
                    .unwrap_or(1.0);
                for process in stats {
                    if let Some((start, ticks)) = sample.processes.get(&process.pid) {
                        if *start == process.start_ticks {
                            let delta = process.ticks.saturating_sub(*ticks);
                            let percent = delta as f64 / total_delta as f64 * cores * 100.0;
                            percents.insert(process.pid, percent.clamp(0.0, 100.0));
                        }
                    }
                }
            }
        }
        sample.at = Some(now);
        sample.total = total;
        sample.processes = stats
            .iter()
            .map(|process| (process.pid, (process.start_ticks, process.ticks)))
            .collect();
        percents
    }

    fn signal(&self, input: &Value) -> Result<Value, Error> {
        let pid = number(input, "pid")
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| Error::invalid("`pid` is required and must be a u32"))?;
        let start_ticks = number(input, "start-ticks")
            .ok_or_else(|| Error::invalid("`start-ticks` is required"))?;
        let signal = text(input, "signal")?.to_ascii_uppercase();
        let signo = SIGNALS
            .iter()
            .find_map(|(name, signo)| (*name == signal).then_some(*signo))
            .ok_or_else(|| Error::invalid(format!("`{signal}` is not allowlisted")))?;

        if pid <= 1 {
            return Err(Error::denied("pid 1 is protected"));
        }
        if pid == std::process::id() {
            return Err(Error::denied("the process service cannot signal itself"));
        }
        let process =
            read_stat(pid).ok_or_else(|| Error::not_found(format!("pid {pid} does not exist")))?;
        if process.start_ticks != start_ticks {
            return Err(Error::conflict(format!(
                "pid {pid} was reused; the signal was not sent"
            )));
        }

        // SAFETY: `kill` has no memory preconditions; the pid and signal are
        // validated above.
        if unsafe { libc::kill(pid as i32, signo) } == 0 {
            *self.cache.lock().expect("the snapshot cache is poisoned") = None;
            Ok(Value::Null)
        } else {
            let error = std::io::Error::last_os_error();
            match error.raw_os_error() {
                Some(libc::EPERM) => Err(Error::denied(format!(
                    "the service cannot signal pid {pid}: {error}"
                ))),
                Some(libc::ESRCH) => Err(Error::not_found(format!("pid {pid} does not exist"))),
                _ => Err(Error::unavailable(format!("signalling pid {pid}: {error}"))),
            }
        }
    }

    fn spawn(&self, input: &Value) -> Result<Value, Error> {
        let arguments = input
            .get("argv")
            .and_then(Value::as_array)
            .ok_or_else(|| Error::invalid("`argv` is a list of strings"))?;
        if arguments.is_empty() || arguments.len() > MAX_ARGV {
            return Err(Error::invalid(format!(
                "`argv` must have between 1 and {MAX_ARGV} entries"
            )));
        }
        let mut argv = Vec::with_capacity(arguments.len());
        for argument in arguments {
            let value = argument
                .as_str()
                .ok_or_else(|| Error::invalid("`argv` is a list of strings"))?;
            if value.len() > MAX_ARGUMENT || value.contains('\0') {
                return Err(Error::invalid("a spawn argument is invalid"));
            }
            argv.push(value.to_string());
        }
        let program = &argv[0];
        if !SPAWN_ALLOWLIST.contains(&program.as_str()) {
            return Err(Error::denied(format!(
                "`{program}` is not in the detached-spawn allowlist"
            )));
        }

        let mut command = Command::new(program);
        command
            .args(&argv[1..])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // SAFETY: `setsid` is async-signal-safe and the closure allocates
        // nothing; it detaches the child from the service's session.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = command
            .spawn()
            .map_err(|error| Error::unavailable(format!("starting `{program}`: {error}")))?;
        let pid = child.id();
        // The service is not init; reap it so a detached helper never becomes a
        // zombie while the service lives.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        *self.cache.lock().expect("the snapshot cache is poisoned") = None;
        Ok(Value::from(pid))
    }
}

impl Handler for Service {
    fn call(&self, interface: &str, function: &str, input: Value) -> Result<Value, Error> {
        if interface != PROC {
            return Err(Error::not_found(format!(
                "`{interface}` is not exported by the process service"
            )));
        }
        match function {
            "list" => self.list(&input),
            "signal" => self.signal(&input),
            "spawn" => self.spawn(&input),
            other => Err(Error::not_found(format!(
                "`anthracite:proc/proc#{other}` is not implemented"
            ))),
        }
    }
}

#[tokio::main]
async fn main() {
    let socket = match anth_service::socket_argument() {
        Ok(socket) => socket,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    let service = Arc::new(Service::new());
    if let Err(error) = anth_service::serve(&socket, service).await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn snapshot() -> Vec<Stat> {
    let Ok(entries) = fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse::<u32>().ok())
        .filter_map(read_stat)
        .collect()
}

fn read_stat(pid: u32) -> Option<Stat> {
    let text = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let open = text.find('(')?;
    let close = text.rfind(')')?;
    let name = text[open + 1..close].to_string();
    let fields: Vec<&str> = text[close + 1..].split_whitespace().collect();
    if fields.len() < 20 {
        return None;
    }
    let state = fields[0].chars().next()?;
    let ppid = fields[1].parse().ok()?;
    let ticks = fields[11].parse::<u64>().ok()? + fields[12].parse::<u64>().ok()?;
    let start_ticks = fields[19].parse().ok()?;
    let memory = resident_bytes(pid);
    let uid = process_uid(pid);
    Some(Stat {
        pid,
        ppid,
        name,
        state,
        ticks,
        start_ticks,
        memory,
        uid,
    })
}

fn resident_bytes(pid: u32) -> u64 {
    let pages = fs::read_to_string(format!("/proc/{pid}/statm"))
        .ok()
        .and_then(|text| {
            text.split_whitespace()
                .nth(1)
                .and_then(|value| value.parse::<u64>().ok())
        })
        .unwrap_or(0);
    // SAFETY: `sysconf` has no memory preconditions.
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    let page = if page > 0 { page as u64 } else { 4096 };
    pages.saturating_mul(page)
}

fn process_uid(pid: u32) -> u32 {
    let Ok(text) = fs::read_to_string(format!("/proc/{pid}/status")) else {
        return 0;
    };
    text.lines()
        .find_map(|line| {
            let value = line.strip_prefix("Uid:")?;
            value.split_whitespace().next()?.parse().ok()
        })
        .unwrap_or(0)
}

fn total_ticks() -> u64 {
    let Ok(text) = fs::read_to_string("/proc/stat") else {
        return 0;
    };
    let Some(line) = text.lines().next() else {
        return 0;
    };
    line.split_whitespace()
        .filter_map(|value| value.parse::<u64>().ok())
        .sum()
}

fn users() -> BTreeMap<u32, String> {
    let Ok(text) = fs::read_to_string("/etc/passwd") else {
        return BTreeMap::new();
    };
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split(':');
            let name = fields.next()?;
            let _password = fields.next()?;
            let uid = fields.next()?.parse::<u32>().ok()?;
            Some((uid, name.to_string()))
        })
        .collect()
}

fn user_name(users: &BTreeMap<u32, String>, uid: u32) -> String {
    users.get(&uid).cloned().unwrap_or_else(|| uid.to_string())
}

fn state_of(state: char) -> &'static str {
    match state {
        'R' | 'I' => "running",
        'T' | 't' => "stopped",
        'Z' | 'X' => "zombie",
        _ => "sleeping",
    }
}

fn text(input: &Value, field: &str) -> Result<String, Error> {
    input
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| Error::invalid(format!("`{field}` is required and must be a string")))
}

fn optional_text(input: &Value, field: &str) -> Option<String> {
    input
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn number(input: &Value, field: &str) -> Option<u64> {
    match input.get(field)? {
        Value::Number(value) => value.as_u64(),
        Value::String(value) => value.parse().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn call(service: &Service, function: &str, input: Value) -> Value {
        service
            .call(PROC, function, input)
            .expect("the service answers")
    }

    #[test]
    fn states_map_to_the_contract() {
        assert_eq!(state_of('R'), "running");
        assert_eq!(state_of('S'), "sleeping");
        assert_eq!(state_of('T'), "stopped");
        assert_eq!(state_of('Z'), "zombie");
    }

    #[test]
    fn the_table_contains_the_test_process() {
        let self_pid = std::process::id();
        let stat = read_stat(self_pid).expect("the test process is in /proc");
        assert_eq!(stat.pid, self_pid);
        let service = Service::new();
        let page = call(&service, "list", json!({"limit": 50}));
        assert!(!page["processes"].as_array().expect("processes").is_empty());
    }

    #[test]
    fn pagination_starts_after_the_cursor() {
        let service = Service::new();
        let first = call(&service, "list", json!({"limit": 1}));
        assert_eq!(first["processes"].as_array().expect("processes").len(), 1);
        let cursor = first["cursor"].as_str().expect("cursor");
        let second = call(&service, "list", json!({"cursor": cursor, "limit": 1}));
        assert_ne!(first["processes"][0]["pid"], second["processes"][0]["pid"]);
    }

    #[test]
    fn a_spawned_process_is_signalled_only_with_its_start_ticks() {
        let service = Service::new();
        let pid = call(&service, "spawn", json!({"argv": ["sleep", "30"]}))
            .as_u64()
            .expect("pid") as u32;
        let start = read_stat(pid).expect("spawned process").start_ticks;
        let error = service
            .call(
                PROC,
                "signal",
                json!({"pid": pid, "start-ticks": start.wrapping_add(999), "signal": "TERM"}),
            )
            .expect_err("wrong start ticks");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Conflict);
        call(
            &service,
            "signal",
            json!({"pid": pid, "start-ticks": start, "signal": "TERM"}),
        );
        std::thread::sleep(Duration::from_millis(200));
    }

    #[test]
    fn init_and_the_service_are_protected() {
        let service = Service::new();
        let init = service
            .call(
                PROC,
                "signal",
                json!({"pid": 1, "start-ticks": 1, "signal": "KILL"}),
            )
            .expect_err("init");
        assert_eq!(init.kind(), anth_contract::ErrorKind::Denied);
        let itself = service
            .call(
                PROC,
                "signal",
                json!({"pid": std::process::id(), "start-ticks": 1, "signal": "KILL"}),
            )
            .expect_err("self");
        assert_eq!(itself.kind(), anth_contract::ErrorKind::Denied);
    }

    #[test]
    fn spawn_is_allowlisted() {
        let service = Service::new();
        let error = service
            .call(PROC, "spawn", json!({"argv": ["rm", "-rf", "/"]}))
            .expect_err("not allowlisted");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Denied);
        let error = service
            .call(PROC, "spawn", json!({"argv": []}))
            .expect_err("empty argv");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Invalid);
    }
}
