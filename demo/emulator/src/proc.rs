//! Deterministic `/proc` fixture for the demo (§19 §2, `proc`).
//!
//! The table is generated from a fixed template: pids, parent links, names,
//! users, states and `start_ticks` never depend on the host. CPU usage moves on
//! a sine of the time since the service started, so the live view looks alive,
//! and the fixture includes one deep chain so the tree guides are exercised.

use std::time::Instant;

use anth_contract::Error;
use serde_json::{json, Value};

use crate::{number, optional_text};

const MAX_PAGE: u32 = 100_000;
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
    started: Instant,
}

#[allow(clippy::too_many_arguments)]
fn process(
    pid: u32,
    ppid: u32,
    name: &str,
    user: &'static str,
    state: &'static str,
    cpu: f64,
    memory: u64,
    start_ticks: u64,
) -> Process {
    Process {
        pid,
        ppid,
        name: name.to_string(),
        user,
        state,
        cpu,
        memory,
        start_ticks,
    }
}

/// A CPU percentage that drifts on a fixed curve, so two snapshots differ
/// without the fixture ever reading the host.
fn cpu_at(process: &Process, elapsed: f64) -> f64 {
    if process.cpu <= 0.0 {
        return 0.0;
    }
    let phase = f64::from(process.pid % 89) * 0.37;
    let value = process.cpu + 0.5 * (elapsed / 4.0 + phase).sin();
    ((value * 10.0).round() / 10.0).clamp(0.0, 100.0)
}

impl ProcFixture {
    pub fn demo() -> Self {
        let mut processes = vec![
            process(1, 0, "init", "root", "running", 0.1, 12_582_912, 1),
            process(2, 0, "kthreadd", "root", "sleeping", 0.0, 0, 2),
            process(
                120,
                1,
                "systemd-journal",
                "root",
                "sleeping",
                0.2,
                25_165_824,
                12,
            ),
            process(160, 1, "syslogd", "root", "sleeping", 0.0, 6_291_456, 16),
            process(
                320,
                1,
                "systemd-udevd",
                "root",
                "sleeping",
                0.0,
                9_437_184,
                32,
            ),
            process(640, 1, "sshd", "root", "sleeping", 0.0, 8_388_608, 64),
            process(
                981,
                1,
                "host-emulator",
                "operator",
                "running",
                2.4,
                33_554_432,
                98,
            ),
            process(
                1204,
                1,
                "anth-panel",
                "operator",
                "running",
                1.1,
                67_108_864,
                120,
            ),
            process(
                1400,
                981,
                "fs-worker",
                "operator",
                "sleeping",
                0.3,
                16_777_216,
                140,
            ),
            process(
                1510,
                981,
                "terminal-worker",
                "operator",
                "sleeping",
                0.1,
                12_582_912,
                151,
            ),
            process(
                1601, 640, "sleep", "operator", "sleeping", 0.0, 4_194_304, 160,
            ),
            process(
                1700,
                1,
                "postgres",
                "postgres",
                "sleeping",
                0.4,
                134_217_728,
                170,
            ),
            process(1800, 2, "kworker/0:1", "root", "running", 0.0, 0, 180),
            process(
                1902, 1204, "bun", "operator", "sleeping", 0.6, 92_274_688, 190,
            ),
            process(2044, 1, "cron", "root", "sleeping", 0.0, 6_291_456, 204),
            process(
                2200,
                1204,
                "manager-worker",
                "operator",
                "sleeping",
                0.2,
                20_971_520,
                220,
            ),
            process(
                2300,
                1204,
                "proc-worker",
                "operator",
                "sleeping",
                0.1,
                18_874_368,
                230,
            ),
        ];

        // One deep chain so the tree view draws continuous guides for more than
        // a couple of levels.
        let mut parent = 2200;
        for level in 1..=7 {
            let pid = 3000 + level;
            let name = if level == 7 {
                "proc-deep-sleep".to_string()
            } else {
                format!("proc-deep-{level}")
            };
            processes.push(process(
                pid,
                parent,
                &name,
                "operator",
                "sleeping",
                if level == 7 { 0.1 } else { 0.0 },
                4_194_304 + u64::from(level) * 524_288,
                u64::from(pid) / 2,
            ));
            parent = pid;
        }

        // A nested service subtree: several siblings under one database.
        for child in 0..6 {
            processes.push(process(
                3500 + child,
                1700,
                &format!("db-writer-{}", child + 1),
                "postgres",
                "sleeping",
                0.2,
                8_388_608 + u64::from(child) * 1_048_576,
                350 + u64::from(child),
            ));
        }

        // A few hundred workers make the desktop virtualization and the header
        // sort meaningful without turning the demo into a screenshot of noise.
        const WORKERS: u32 = 240;
        let parents = [981, 1204, 1700, 2044, 640];
        let users = ["operator", "www-data", "postgres", "root"];
        for index in 0..WORKERS {
            let pid = 4000 + index;
            let state = if index % 37 == 0 {
                "zombie"
            } else if index % 9 == 0 {
                "running"
            } else {
                "sleeping"
            };
            let cpu = if index % 5 == 0 {
                0.1 + f64::from(index % 7) * 0.1
            } else {
                0.0
            };
            processes.push(process(
                pid,
                parents[index as usize % parents.len()],
                &format!("worker-{:03}", index + 1),
                users[index as usize % users.len()],
                state,
                cpu,
                3_145_728 + u64::from(index % 13) * 1_048_576,
                400 + u64::from(index),
            ));
        }

        Self {
            processes,
            started: Instant::now(),
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
                let elapsed = self.started.elapsed().as_secs_f64();
                let processes: Vec<Value> = self.processes[start..end]
                    .iter()
                    .map(|process| {
                        json!({
                            "pid": process.pid,
                            "ppid": process.ppid,
                            "name": process.name,
                            "user": process.user,
                            "state": process.state,
                            "cpu-percent": cpu_at(process, elapsed),
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
        assert_eq!(page["processes"][1]["pid"], 2);
        assert_eq!(page["processes"][2]["pid"], 120);
        assert_eq!(page["done"], false);
        let cursor = page["cursor"].as_str().expect("cursor");
        let next = call(&mut fixture, "list", json!({"cursor": cursor, "limit": 3}));
        assert_eq!(next["processes"][0]["pid"], 160);
    }

    #[test]
    fn the_whole_table_fits_in_one_page_and_has_a_deep_chain() {
        let mut fixture = ProcFixture::demo();
        let page = call(&mut fixture, "list", json!({"limit": 100_000}));
        let processes = page["processes"].as_array().expect("processes");
        assert!(processes.len() > 250, "the demo needs enough rows");
        assert_eq!(page["done"], true);
        let deep = processes
            .iter()
            .find(|process| process["name"] == "proc-deep-sleep")
            .expect("the deep chain");
        assert_eq!(deep["ppid"], 3006);
        for process in processes {
            let cpu = process["cpu-percent"].as_f64().expect("cpu");
            assert!((0.0..=100.0).contains(&cpu), "cpu out of range: {cpu}");
        }
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
        let page = call(&mut fixture, "list", json!({"limit": 100_000}));
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
        assert!(pid > 4239);
        let page = call(&mut fixture, "list", json!({"limit": 100_000}));
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
