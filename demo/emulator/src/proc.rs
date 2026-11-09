//! Deterministic `/proc` fixture for the demo (§19 §2, `proc`).

use anth_contract::Error;
use serde_json::{json, Value};

use crate::{number, optional_text};

const MAX_PAGE: u32 = 500;
const DEFAULT_PAGE: u32 = 50;

#[derive(Clone)]
struct Process {
    pid: u32,
    ppid: u32,
    name: String,
    user: &'static str,
    state: &'static str,
    cpu: f64,
    memory: u64,
    start_ticks: u64,
}

/// A stable process table: pids, ticks and counters never depend on the host.
pub struct ProcFixture {
    processes: Vec<Process>,
}

impl ProcFixture {
    pub fn demo() -> Self {
        Self {
            processes: vec![
                Process {
                    pid: 1,
                    ppid: 0,
                    name: "init".to_string(),
                    user: "root",
                    state: "running",
                    cpu: 0.1,
                    memory: 12_582_912,
                    start_ticks: 1,
                },
                Process {
                    pid: 120,
                    ppid: 1,
                    name: "systemd-journal".to_string(),
                    user: "root",
                    state: "sleeping",
                    cpu: 0.2,
                    memory: 25_165_824,
                    start_ticks: 12,
                },
                Process {
                    pid: 640,
                    ppid: 1,
                    name: "sshd".to_string(),
                    user: "root",
                    state: "sleeping",
                    cpu: 0.0,
                    memory: 8_388_608,
                    start_ticks: 64,
                },
                Process {
                    pid: 981,
                    ppid: 1,
                    name: "host-emulator".to_string(),
                    user: "operator",
                    state: "running",
                    cpu: 2.4,
                    memory: 33_554_432,
                    start_ticks: 98,
                },
                Process {
                    pid: 1204,
                    ppid: 1,
                    name: "anth-panel".to_string(),
                    user: "operator",
                    state: "running",
                    cpu: 1.1,
                    memory: 67_108_864,
                    start_ticks: 120,
                },
                Process {
                    pid: 1400,
                    ppid: 981,
                    name: "fs-worker".to_string(),
                    user: "operator",
                    state: "sleeping",
                    cpu: 0.3,
                    memory: 16_777_216,
                    start_ticks: 140,
                },
                Process {
                    pid: 1510,
                    ppid: 981,
                    name: "terminal-worker".to_string(),
                    user: "operator",
                    state: "sleeping",
                    cpu: 0.1,
                    memory: 12_582_912,
                    start_ticks: 151,
                },
                Process {
                    pid: 1601,
                    ppid: 640,
                    name: "sleep".to_string(),
                    user: "operator",
                    state: "sleeping",
                    cpu: 0.0,
                    memory: 4_194_304,
                    start_ticks: 160,
                },
                Process {
                    pid: 1700,
                    ppid: 1,
                    name: "postgres".to_string(),
                    user: "postgres",
                    state: "sleeping",
                    cpu: 0.4,
                    memory: 134_217_728,
                    start_ticks: 170,
                },
                Process {
                    pid: 1800,
                    ppid: 2,
                    name: "kworker/0:1".to_string(),
                    user: "root",
                    state: "running",
                    cpu: 0.0,
                    memory: 0,
                    start_ticks: 180,
                },
                Process {
                    pid: 1902,
                    ppid: 1204,
                    name: "bun".to_string(),
                    user: "operator",
                    state: "sleeping",
                    cpu: 0.6,
                    memory: 92_274_688,
                    start_ticks: 190,
                },
                Process {
                    pid: 2044,
                    ppid: 1,
                    name: "cron".to_string(),
                    user: "root",
                    state: "sleeping",
                    cpu: 0.0,
                    memory: 6_291_456,
                    start_ticks: 204,
                },
            ],
        }
    }

    fn spawn(&mut self, argv: Vec<String>) -> Result<u32, Error> {
        if argv.is_empty() || argv.len() > 4 {
            return Err(Error::invalid("`argv` must have between 1 and 4 entries"));
        }
        for argument in &argv {
            if argument.len() > 128 || argument.contains('\0') {
                return Err(Error::invalid("a spawn argument is invalid"));
            }
        }
        let base = argv[0].rsplit('/').next().unwrap_or(&argv[0]).to_string();
        if !matches!(base.as_str(), "sleep" | "echo" | "true") {
            return Err(Error::denied(format!(
                "`{base}` is not in the detached-spawn allowlist"
            )));
        }
        let pid = self
            .processes
            .iter()
            .map(|process| process.pid)
            .max()
            .unwrap_or(0)
            + 1;
        self.processes.push(Process {
            pid,
            ppid: 1,
            name: base,
            user: "operator",
            state: "running",
            cpu: 0.0,
            memory: 1_048_576,
            start_ticks: u64::from(pid) + 1_000,
        });
        Ok(pid)
    }

    pub fn handle(&mut self, function: &str, input: &Value) -> Result<Value, Error> {
        match function {
            "list" => {
                let start = optional_text(input, "cursor")
                    .map(|cursor| cursor.parse::<usize>())
                    .transpose()
                    .map_err(|_| Error::invalid("`cursor` is not a page cursor"))?
                    .unwrap_or(0)
                    .min(self.processes.len());
                let limit = number(input, "limit")
                    .unwrap_or(u64::from(DEFAULT_PAGE))
                    .clamp(1, u64::from(MAX_PAGE)) as usize;
                let end = (start + limit).min(self.processes.len());
                let processes: Vec<Value> = self.processes[start..end]
                    .iter()
                    .map(|process| {
                        json!({
                            "pid": process.pid,
                            "ppid": process.ppid,
                            "name": process.name,
                            "user": process.user,
                            "state": process.state,
                            "cpu-percent": process.cpu,
                            "memory-bytes": process.memory,
                            "start-ticks": process.start_ticks,
                        })
                    })
                    .collect();
                Ok(json!({
                    "processes": processes,
                    "cursor": (end < self.processes.len()).then(|| (end as u64).to_string()),
                    "done": end >= self.processes.len(),
                }))
            }
            "signal" => {
                let pid = number(input, "pid")
                    .and_then(|value| u32::try_from(value).ok())
                    .ok_or_else(|| Error::invalid("`pid` is required and must be a u32"))?;
                let start_ticks = number(input, "start-ticks")
                    .ok_or_else(|| Error::invalid("`start-ticks` is required"))?;
                let signal = crate::text(input, "signal")?.to_ascii_uppercase();
                if !matches!(
                    signal.as_str(),
                    "HUP" | "INT" | "TERM" | "KILL" | "STOP" | "CONT"
                ) {
                    return Err(Error::invalid(format!("`{signal}` is not allowlisted")));
                }
                if pid == 1 {
                    return Err(Error::denied("pid 1 is protected"));
                }
                let process = self
                    .processes
                    .iter_mut()
                    .find(|process| process.pid == pid)
                    .ok_or_else(|| Error::not_found(format!("pid {pid} does not exist")))?;
                if process.start_ticks != start_ticks {
                    return Err(Error::conflict(format!(
                        "pid {pid} was reused; the signal was not sent"
                    )));
                }
                process.state = match signal.as_str() {
                    "TERM" | "KILL" | "STOP" => "stopped",
                    "CONT" => "running",
                    _ => process.state,
                };
                if signal == "KILL" {
                    process.state = "zombie";
                }
                Ok(Value::Null)
            }
            "spawn" => {
                let arguments = input
                    .get("argv")
                    .and_then(Value::as_array)
                    .ok_or_else(|| Error::invalid("`argv` is a list of strings"))?;
                let mut argv = Vec::with_capacity(arguments.len());
                for argument in arguments {
                    argv.push(
                        argument
                            .as_str()
                            .ok_or_else(|| Error::invalid("`argv` is a list of strings"))?
                            .to_string(),
                    );
                }
                Ok(Value::from(self.spawn(argv)?))
            }
            other => Err(Error::not_found(format!(
                "`anthracite:proc/proc#{other}` is not implemented"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(fixture: &mut ProcFixture, function: &str, input: Value) -> Value {
        fixture
            .handle(function, &input)
            .expect("the fixture answers")
    }

    #[test]
    fn the_table_is_stable_and_paginates() {
        let mut fixture = ProcFixture::demo();
        let page = call(&mut fixture, "list", json!({"limit": 3}));
        assert_eq!(page["processes"][0]["pid"], 1);
        assert_eq!(page["processes"][1]["pid"], 120);
        assert_eq!(page["processes"][2]["pid"], 640);
        assert_eq!(page["done"], false);
        let cursor = page["cursor"].as_str().expect("cursor");
        let next = call(&mut fixture, "list", json!({"cursor": cursor, "limit": 3}));
        assert_eq!(next["processes"][0]["pid"], 981);
    }

    #[test]
    fn a_signal_needs_the_start_ticks() {
        let mut fixture = ProcFixture::demo();
        let error = fixture
            .handle(
                "signal",
                &json!({"pid": 640, "start-ticks": 1, "signal": "TERM"}),
            )
            .expect_err("reused pid");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Conflict);
        call(
            &mut fixture,
            "signal",
            json!({"pid": 640, "start-ticks": 64, "signal": "TERM"}),
        );
        let page = call(&mut fixture, "list", json!({"limit": 500}));
        let process = page["processes"]
            .as_array()
            .expect("processes")
            .iter()
            .find(|process| process["pid"] == 640)
            .expect("pid 640");
        assert_eq!(process["state"], "stopped");
    }

    #[test]
    fn init_is_protected() {
        let mut fixture = ProcFixture::demo();
        let error = fixture
            .handle(
                "signal",
                &json!({"pid": 1, "start-ticks": 1, "signal": "KILL"}),
            )
            .expect_err("init");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Denied);
    }

    #[test]
    fn spawn_is_allowlisted_and_adds_a_process() {
        let mut fixture = ProcFixture::demo();
        let pid = call(&mut fixture, "spawn", json!({"argv": ["sleep", "30"]}));
        let pid = pid.as_u64().expect("pid") as u32;
        assert!(pid > 2044);
        let page = call(&mut fixture, "list", json!({"limit": 500}));
        let process = page["processes"]
            .as_array()
            .expect("processes")
            .iter()
            .find(|process| process["pid"] == pid)
            .expect("spawned");
        assert_eq!(process["name"], "sleep");

        let error = fixture
            .handle("spawn", &json!({"argv": ["rm", "-rf", "/"]}))
            .expect_err("not allowlisted");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Denied);
        let error = fixture
            .handle("spawn", &json!({"argv": []}))
            .expect_err("empty");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Invalid);
    }
}
