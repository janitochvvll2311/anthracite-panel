//! Native Terminal service: one PTY per session.
//!
//! The service runs the requested shell in a pseudo-terminal, reads its master
//! side on a thread and exposes the contract's chunked `read` over WIT-RPC.
//! Resize and signals operate on the session's process group, so closing a
//! session ends the shell and its children.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use anth_contract::Error;
use anth_service::Handler;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde_json::{json, Value};

const TERMINAL: &str = "anthracite:terminal/terminal@1.0.0";
const MAX_DIMENSION: u32 = 500;
const MAX_WAIT_MS: u64 = 30_000;
const MAX_BUFFER: usize = 4 * 1024 * 1024;

struct Service {
    inner: Mutex<Inner>,
}

struct Inner {
    next: u64,
    sessions: BTreeMap<String, Session>,
}

struct Session {
    shell: String,
    owner: String,
    master: Box<dyn MasterPty + Send>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    child: Box<dyn Child + Send + Sync>,
    output: Arc<Output>,
    cols: u16,
    rows: u16,
}

struct Output {
    state: Mutex<OutputState>,
    ready: Condvar,
}

struct OutputState {
    chunks: Vec<(u64, Vec<u8>)>,
    next_seq: u64,
    eof: bool,
    bytes: usize,
}

impl Output {
    fn new() -> Self {
        Self {
            state: Mutex::new(OutputState {
                chunks: Vec::new(),
                next_seq: 0,
                eof: false,
                bytes: 0,
            }),
            ready: Condvar::new(),
        }
    }

    fn push(&self, data: Vec<u8>) {
        let mut state = self.state.lock().expect("terminal output mutex");
        let seq = state.next_seq;
        state.next_seq += 1;
        state.bytes += data.len();
        state.chunks.push((seq, data));
        while state.bytes > MAX_BUFFER && !state.chunks.is_empty() {
            let (_, dropped) = state.chunks.remove(0);
            state.bytes = state.bytes.saturating_sub(dropped.len());
        }
        self.ready.notify_all();
    }

    fn finish(&self) {
        let mut state = self.state.lock().expect("terminal output mutex");
        state.eof = true;
        self.ready.notify_all();
    }

    fn last_seq(&self) -> u64 {
        let state = self.state.lock().expect("terminal output mutex");
        state.next_seq.saturating_sub(1)
    }

    fn read(&self, id: &str, cursor: u64, wait_ms: u64) -> Result<Option<Value>, Error> {
        let mut state = self.state.lock().expect("terminal output mutex");
        let deadline = wait_ms.min(MAX_WAIT_MS);
        loop {
            if let Some(index) = state.chunks.iter().position(|(seq, _)| *seq >= cursor) {
                let (seq, data) = state.chunks.remove(index);
                state.chunks.retain(|(candidate, _)| *candidate > seq);
                state.bytes = state.chunks.iter().map(|(_, data)| data.len()).sum();
                return Ok(Some(json!({
                    "seq": seq,
                    "data": data,
                })));
            }
            if state.eof {
                return Err(Error::not_found(format!("session `{id}` has ended")));
            }
            if deadline == 0 {
                return Ok(None);
            }
            let (next, timed_out) = self
                .ready
                .wait_timeout(state, Duration::from_millis(deadline))
                .map_err(|_| Error::internal("terminal output wait was poisoned"))?;
            state = next;
            if timed_out.timed_out() {
                return Ok(None);
            }
        }
    }
}

impl Service {
    fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                next: 0,
                sessions: BTreeMap::new(),
            }),
        }
    }

    fn open(&self, input: &Value) -> Result<Value, Error> {
        let shell = text(input, "shell")?;
        let cols = dimension(input, "cols")?;
        let rows = dimension(input, "rows")?;
        if !Path::new(&shell).is_absolute() {
            return Err(Error::invalid("`shell` must be an absolute path"));
        }
        if !executable(&shell) {
            return Err(Error::not_found(format!(
                "`{shell}` is not an executable file"
            )));
        }

        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| Error::unavailable(format!("opening a PTY: {error}")))?;
        let mut command = CommandBuilder::new(&shell);
        command.env("TERM", "dumb");
        command.env("NO_COLOR", "1");
        command.env("CLICOLOR", "0");
        command.env("PS1", "$ ");
        command.env("PROMPT", "$ ");
        command.env("LANG", "C.UTF-8");
        command.env("LC_ALL", "C.UTF-8");
        if let Ok(home) = std::env::var("HOME") {
            if Path::new(&home).is_dir() {
                command.cwd(home);
            }
        }
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|error| Error::unavailable(format!("starting `{shell}`: {error}")))?;
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| Error::unavailable(format!("opening the PTY reader: {error}")))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|error| Error::unavailable(format!("opening the PTY writer: {error}")))?;
        let master = pair.master;

        let output = Arc::new(Output::new());
        let output_thread = Arc::clone(&output);
        std::thread::spawn(move || {
            let mut reader = reader;
            let mut buffer = [0u8; 8192];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(read) => output_thread.push(buffer[..read].to_vec()),
                    Err(_) => break,
                }
            }
            output_thread.finish();
        });

        let mut inner = self.inner.lock().expect("terminal sessions mutex");
        inner.next += 1;
        let id = format!("t{}", inner.next);
        let owner = std::env::var("USER")
            .or_else(|_| std::env::var("LOGNAME"))
            .unwrap_or_else(|_| "operator".to_string());
        let session = Session {
            shell: shell.clone(),
            owner: owner.clone(),
            master,
            writer: Arc::new(Mutex::new(writer)),
            child,
            output: Arc::clone(&output),
            cols,
            rows,
        };
        let view = json!({
            "id": id,
            "shell": shell,
            "owner": owner,
            "seq": output.last_seq(),
        });
        inner.sessions.insert(id, session);
        Ok(view)
    }

    fn list(&self) -> Result<Value, Error> {
        let inner = self.inner.lock().expect("terminal sessions mutex");
        Ok(Value::Array(
            inner
                .sessions
                .iter()
                .map(|(id, session)| {
                    json!({
                        "id": id,
                        "shell": session.shell,
                        "owner": session.owner,
                        "seq": session.output.last_seq(),
                    })
                })
                .collect(),
        ))
    }

    fn read(&self, input: &Value) -> Result<Value, Error> {
        let id = text(input, "id")?;
        let cursor = number(input, "seq").unwrap_or(0);
        let wait = number(input, "wait-ms").unwrap_or(0);
        let output = {
            let inner = self.inner.lock().expect("terminal sessions mutex");
            let session = inner
                .sessions
                .get(&id)
                .ok_or_else(|| Error::not_found(format!("session `{id}` does not exist")))?;
            Arc::clone(&session.output)
        };
        Ok(output.read(&id, cursor, wait)?.unwrap_or(Value::Null))
    }

    fn write(&self, input: &Value) -> Result<Value, Error> {
        let id = text(input, "id")?;
        let data = bytes(input.get("data").unwrap_or(&Value::Null))?;
        let writer = {
            let inner = self.inner.lock().expect("terminal sessions mutex");
            let session = inner
                .sessions
                .get(&id)
                .ok_or_else(|| Error::not_found(format!("session `{id}` does not exist")))?;
            Arc::clone(&session.writer)
        };
        let mut writer = writer
            .lock()
            .map_err(|_| Error::internal("the terminal writer lock was poisoned"))?;
        writer
            .write_all(&data)
            .and_then(|_| writer.flush())
            .map_err(|error| Error::unavailable(format!("writing to `{id}`: {error}")))?;
        Ok(Value::Null)
    }

    fn resize(&self, input: &Value) -> Result<Value, Error> {
        let id = text(input, "id")?;
        let cols = dimension(input, "cols")?;
        let rows = dimension(input, "rows")?;
        let mut inner = self.inner.lock().expect("terminal sessions mutex");
        let session = inner
            .sessions
            .get_mut(&id)
            .ok_or_else(|| Error::not_found(format!("session `{id}` does not exist")))?;
        session
            .master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| Error::unavailable(format!("resizing `{id}`: {error}")))?;
        session.cols = cols;
        session.rows = rows;
        Ok(Value::Null)
    }

    fn signal(&self, input: &Value) -> Result<Value, Error> {
        let id = text(input, "id")?;
        let name = text(input, "signal")?;
        let signal = signal_number(&name)?;
        let inner = self.inner.lock().expect("terminal sessions mutex");
        let session = inner
            .sessions
            .get(&id)
            .ok_or_else(|| Error::not_found(format!("session `{id}` does not exist")))?;
        signal_group(session, signal)?;
        Ok(Value::Null)
    }

    fn close(&self, input: &Value) -> Result<Value, Error> {
        let id = text(input, "id")?;
        let mut session = {
            let mut inner = self.inner.lock().expect("terminal sessions mutex");
            inner
                .sessions
                .remove(&id)
                .ok_or_else(|| Error::not_found(format!("session `{id}` does not exist")))?
        };
        let _ = signal_group(&session, libc::SIGKILL);
        let _ = session.child.kill();
        let _ = session.child.wait();
        Ok(Value::Null)
    }
}

impl Handler for Service {
    fn call(&self, interface: &str, function: &str, input: Value) -> Result<Value, Error> {
        if interface != TERMINAL {
            return Err(Error::not_found(format!(
                "`{interface}` is not exported by the terminal service"
            )));
        }
        match function {
            "open" => self.open(&input),
            "list" => self.list(),
            "read" => self.read(&input),
            "write" => self.write(&input),
            "resize" => self.resize(&input),
            "signal" => self.signal(&input),
            "close" => self.close(&input),
            other => Err(Error::not_found(format!(
                "`anthracite:terminal/terminal#{other}` is not implemented"
            ))),
        }
    }
}

fn text(input: &Value, field: &str) -> Result<String, Error> {
    input
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| Error::invalid(format!("`{field}` is required and must be a string")))
}

fn number(input: &Value, field: &str) -> Option<u64> {
    match input.get(field)? {
        Value::Number(value) => value.as_u64(),
        Value::String(value) => value.parse().ok(),
        _ => None,
    }
}

fn dimension(input: &Value, field: &str) -> Result<u16, Error> {
    let value = number(input, field).unwrap_or(0);
    if value == 0 {
        return Err(Error::invalid(format!(
            "`{field}` must be greater than zero"
        )));
    }
    Ok(value.clamp(1, u64::from(MAX_DIMENSION)) as u16)
}

fn bytes(input: &Value) -> Result<Vec<u8>, Error> {
    let values = input
        .as_array()
        .ok_or_else(|| Error::invalid("`data` is a list of u8"))?;
    values
        .iter()
        .map(|value| {
            value
                .as_u64()
                .filter(|value| *value <= u64::from(u8::MAX))
                .map(|value| value as u8)
                .ok_or_else(|| Error::invalid("`data` holds a value that is not a u8"))
        })
        .collect()
}

fn executable(path: &str) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn signal_number(name: &str) -> Result<i32, Error> {
    match name {
        "HUP" => Ok(libc::SIGHUP),
        "INT" => Ok(libc::SIGINT),
        "TERM" => Ok(libc::SIGTERM),
        "KILL" => Ok(libc::SIGKILL),
        "WINCH" => Ok(libc::SIGWINCH),
        other => Err(Error::invalid(format!(
            "`{other}` is not an allowlisted signal"
        ))),
    }
}

fn signal_group(session: &Session, signal: i32) -> Result<(), Error> {
    #[cfg(unix)]
    {
        let pid = session
            .master
            .process_group_leader()
            .or_else(|| session.child.process_id().map(|pid| pid as libc::pid_t));
        let Some(pid) = pid.filter(|pid| *pid > 0) else {
            return Err(Error::unavailable(
                "the terminal session has no process group",
            ));
        };
        let result = unsafe { libc::kill(-pid, signal) };
        if result == 0 {
            Ok(())
        } else {
            Err(Error::unavailable(format!(
                "signalling process group {pid}: {}",
                std::io::Error::last_os_error()
            )))
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (session, signal);
        Err(Error::unavailable(
            "signals are not supported on this platform",
        ))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn call(service: &Service, function: &str, input: Value) -> Value {
        service
            .call(TERMINAL, function, input)
            .expect("terminal call")
    }

    #[test]
    fn a_shell_round_trip_and_close() {
        let service = Service::new();
        let session = call(
            &service,
            "open",
            json!({"shell": "/bin/sh", "cols": 80, "rows": 24}),
        );
        let id = session["id"].as_str().expect("id");
        call(
            &service,
            "write",
            json!({"id": id, "data": b"echo terminal-native\r".to_vec()}),
        );
        let mut output = Vec::new();
        for _ in 0..20 {
            let value = call(
                &service,
                "read",
                json!({"id": id, "seq": 0, "wait-ms": 200}),
            );
            if value.is_null() {
                continue;
            }
            if let Some(data) = value.get("data").and_then(Value::as_array) {
                output.extend(data.iter().filter_map(Value::as_u64).map(|byte| byte as u8));
            }
            if String::from_utf8_lossy(&output).contains("terminal-native") {
                break;
            }
        }
        assert!(String::from_utf8_lossy(&output).contains("terminal-native"));
        call(&service, "close", json!({"id": id}));
        let error = service
            .call(TERMINAL, "read", json!({"id": id, "seq": 0, "wait-ms": 0}))
            .expect_err("closed");
        assert_eq!(error.kind(), anth_contract::ErrorKind::NotFound);
    }

    #[test]
    fn a_relative_shell_is_refused() {
        let service = Service::new();
        let error = service
            .call(
                TERMINAL,
                "open",
                json!({"shell": "sh", "cols": 80, "rows": 24}),
            )
            .expect_err("relative");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Invalid);
    }
}
