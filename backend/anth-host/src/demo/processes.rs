//! In-memory process table for the demo build.

use std::sync::Mutex;

use anth_wire::{
    ErrorKind, Page, PageRequest, ProcessInfo, ProcessSignal, ProcessSnapshot, ProcessStarted,
    WireError,
};

use crate::backend::ProcessesService;
use crate::now_ms;

pub struct DemoProcesses {
    control: bool,
    spawn: bool,
    spawn_allow: Vec<String>,
    state: Mutex<DemoState>,
}

struct DemoState {
    processes: Vec<ProcessInfo>,
    next_pid: u32,
}

impl DemoProcesses {
    pub fn new(control: bool, spawn: bool, spawn_allow: Vec<String>) -> Self {
        Self {
            control,
            spawn,
            spawn_allow,
            state: Mutex::new(seed()),
        }
    }
}

impl ProcessesService for DemoProcesses {
    fn snapshot(&self, request: &PageRequest) -> Result<ProcessSnapshot, WireError> {
        if !request.valid() {
            return Err(WireError::invalid(
                "the page limit or cursor is out of bounds",
            ));
        }
        let offset = match request.cursor.as_deref() {
            None => 0,
            Some(cursor) => cursor
                .parse::<usize>()
                .map_err(|_| WireError::invalid("the process page cursor is not valid"))?,
        };
        let state = self.state.lock().expect("demo process lock");
        let mut processes = state.processes.clone();
        processes.sort_by(|a, b| b.cpu_ticks.cmp(&a.cpu_ticks).then(a.pid.cmp(&b.pid)));
        let total = processes.len();
        let limit = usize::from(request.limit());
        let end = offset.saturating_add(limit).min(total);
        let items = processes
            .get(offset..end)
            .map(<[ProcessInfo]>::to_vec)
            .unwrap_or_default();
        Ok(ProcessSnapshot {
            page: Page {
                items,
                next: (end < total).then(|| end.to_string()),
                total: Some(u64::try_from(total).unwrap_or(u64::MAX)),
            },
            clock_ticks: 100,
            timestamp_ms: now_ms(),
            control_enabled: self.control,
            spawn_enabled: self.spawn,
            spawn_allow: self.spawn_allow.clone(),
        })
    }

    fn signal(
        &self,
        pid: u32,
        start_ticks: u64,
        signal: ProcessSignal,
        group: bool,
    ) -> Result<(), WireError> {
        if !self.control {
            return Err(WireError::new(
                ErrorKind::Forbidden,
                "process_control_disabled",
                "process control is disabled; set `[process] control = true`",
            ));
        }
        if pid == 1 {
            return Err(WireError::new(
                ErrorKind::Forbidden,
                "protected_process",
                "pid 1 is protected",
            ));
        }
        let mut state = self.state.lock().expect("demo process lock");
        let index = state
            .processes
            .iter()
            .position(|process| process.pid == pid && process.start_ticks == start_ticks)
            .ok_or_else(|| {
                WireError::new(
                    ErrorKind::NotFound,
                    "process_not_found",
                    format!("no process {pid} with start time {start_ticks}"),
                )
            })?;
        match signal {
            ProcessSignal::Kill | ProcessSignal::Term => {
                state.processes.remove(index);
            }
            ProcessSignal::Stop => {
                state.processes[index].state = "T".to_string();
            }
            ProcessSignal::Cont => {
                state.processes[index].state = "S".to_string();
                state.processes[index].command =
                    format!("{} (resumed)", state.processes[index].command);
            }
            _ => {
                state.processes[index].command =
                    format!("{} (signal)", state.processes[index].command);
            }
        }
        if group && index < state.processes.len() {
            state.processes[index].command = format!("{} (group)", state.processes[index].command);
        }
        Ok(())
    }

    fn spawn(
        &self,
        program: &str,
        args: &[String],
        _cwd: Option<&str>,
    ) -> Result<ProcessStarted, WireError> {
        if !self.spawn {
            return Err(WireError::new(
                ErrorKind::Forbidden,
                "process_spawn_disabled",
                "process spawning is disabled; set `[process] spawn = true`",
            ));
        }
        if !self.spawn_allow.iter().any(|allowed| allowed == program) {
            return Err(WireError::new(
                ErrorKind::Forbidden,
                "program_not_allowed",
                format!("`{program}` is not in the spawn allowlist"),
            ));
        }
        let mut state = self.state.lock().expect("demo process lock");
        let pid = state.next_pid;
        state.next_pid += 1;
        let command = std::iter::once(program)
            .chain(args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ");
        state.processes.push(ProcessInfo {
            pid,
            ppid: 1,
            uid: 1000,
            user: "demo".to_string(),
            name: program.rsplit('/').next().unwrap_or(program).to_string(),
            command,
            state: "S".to_string(),
            cpu_ticks: 0,
            rss_kib: 2048,
            threads: 1,
            start_ticks: u64::from(pid) * 100,
        });
        Ok(ProcessStarted {
            pid,
            program: program.to_string(),
            args: args.to_vec(),
        })
    }
}

#[allow(clippy::too_many_arguments)]
fn process(
    pid: u32,
    ppid: u32,
    uid: u32,
    user: &str,
    name: &str,
    command: &str,
    state: &str,
    cpu: u64,
    rss: u64,
    threads: u32,
) -> ProcessInfo {
    ProcessInfo {
        pid,
        ppid,
        uid,
        user: user.to_string(),
        name: name.to_string(),
        command: command.to_string(),
        state: state.to_string(),
        cpu_ticks: cpu,
        rss_kib: rss,
        threads,
        start_ticks: u64::from(pid) * 100,
    }
}

fn seed() -> DemoState {
    DemoState {
        processes: vec![
            process(1, 0, 0, "root", "init", "/sbin/init", "S", 120, 4096, 1),
            process(
                412,
                1,
                0,
                "root",
                "sshd",
                "/usr/sbin/sshd -D",
                "S",
                90,
                6144,
                1,
            ),
            process(
                921,
                1,
                0,
                "root",
                "anth-panel",
                "/usr/local/bin/anth-panel run --config /etc/anth/config.toml",
                "S",
                80,
                12_288,
                4,
            ),
            process(
                1204,
                921,
                0,
                "root",
                "demo-worker",
                "/usr/bin/demo-worker --queue default",
                "S",
                30,
                8192,
                2,
            ),
            process(1440, 1, 1000, "demo", "sleep", "sleep 30", "S", 2, 1024, 1),
        ],
        next_pid: 2000,
    }
}
