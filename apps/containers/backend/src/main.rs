//! Docker-compatible container manager for Anthracite.
//!
//! The service speaks the Engine API over the runtime socket named in argv. It
//! never shells out, never receives a host path through WIT, and refuses the
//! create shapes that would escape the runtime's own isolation.

mod model;
mod runtime;

use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream as StdUnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anth_contract::Error;
use anth_service::{ChunkStream, Handler};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::unix::OwnedWriteHalf;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use runtime::{Decoder, Runtime};

const RUNTIME: &str = "anthracite:containers/runtime@1.0.0";
const AUDIT: &str = "anth:audit/record@1.0.0";

struct Session {
    exec: String,
    writer: Arc<tokio::sync::Mutex<OwnedWriteHalf>>,
    receiver: Option<mpsc::Receiver<Vec<u8>>>,
    task: JoinHandle<()>,
}

struct Service {
    runtime: Runtime,
    broker: Option<PathBuf>,
    handle: tokio::runtime::Handle,
    sessions: Mutex<HashMap<String, Session>>,
    counter: AtomicU64,
}

impl Service {
    fn audit(&self, action: &str, detail: Value) {
        let Some(path) = self.broker.as_ref() else {
            return;
        };
        let Ok(body) = serde_json::to_vec(&json!({
            "interface": AUDIT,
            "function": "record",
            "input": {"action": action, "detail": detail},
        })) else {
            return;
        };
        let Ok(mut stream) = StdUnixStream::connect(path) else {
            return;
        };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
        let head = format!(
            "POST /__anth/call HTTP/1.1\r\nHost: anthracite\r\n\
             Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(&body);
        let _ = stream.flush();
        let mut answer = Vec::new();
        let _ = stream.read_to_end(&mut answer);
    }

    fn lifecycle(&self, input: &Value, action: &str) -> Result<Value, Error> {
        let id = text(input, "id")?;
        let timeout = input
            .get("timeout-s")
            .and_then(Value::as_u64)
            .map(|value| value.min(600) as u32);
        self.handle
            .block_on(self.runtime.lifecycle(id, action, timeout))?;
        self.audit(&format!("containers.{action}"), json!({"id": id}));
        Ok(json!(true))
    }

    fn exec(&self, input: &Value) -> Result<Value, Error> {
        let id = text(input, "id")?.to_string();
        let command: Vec<String> = input
            .get("command")
            .and_then(Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|entry| entry.as_str().map(|value| value.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        if command.is_empty() {
            return Err(Error::invalid("an exec needs a command"));
        }
        let tty = flag(input, "tty");
        let cols = input
            .get("cols")
            .and_then(Value::as_u64)
            .unwrap_or(80)
            .clamp(1, 500) as u32;
        let rows = input
            .get("rows")
            .and_then(Value::as_u64)
            .unwrap_or(24)
            .clamp(1, 500) as u32;
        let exec = self
            .handle
            .block_on(self.runtime.exec_create(&id, &command, tty))?;
        let (reader, writer) = self.handle.block_on(self.runtime.exec_start(&exec, tty))?;
        self.handle
            .block_on(self.runtime.exec_resize(&exec, cols, rows))?;
        let (sender, receiver) = mpsc::channel(256);
        let task = self.handle.spawn(async move {
            let mut reader = reader;
            let mut decoder = Decoder::new(tty);
            let mut buffer = [0u8; 4096];
            loop {
                match reader.read(&mut buffer).await {
                    Ok(0) => break,
                    Ok(read) => {
                        for chunk in decoder.push(&buffer[..read]) {
                            if sender.send(chunk).await.is_err() {
                                return;
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        let session = format!(
            "{:x}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|time| time.as_nanos())
                .unwrap_or(0),
            self.counter.fetch_add(1, Ordering::SeqCst)
        );
        self.sessions.lock().expect("sessions lock").insert(
            session.clone(),
            Session {
                exec,
                writer: Arc::new(tokio::sync::Mutex::new(writer)),
                receiver: Some(receiver),
                task,
            },
        );
        Ok(json!(session))
    }

    fn exec_write(&self, input: &Value) -> Result<Value, Error> {
        let id = text(input, "session")?;
        let data: Vec<u8> = input
            .get("data")
            .and_then(Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(Value::as_u64)
                    .map(|value| value as u8)
                    .collect()
            })
            .unwrap_or_default();
        let writer = {
            let sessions = self.sessions.lock().expect("sessions lock");
            sessions
                .get(id)
                .map(|session| Arc::clone(&session.writer))
                .ok_or_else(|| Error::not_found(format!("no exec session `{id}`")))?
        };
        self.handle.block_on(async move {
            let mut writer = writer.lock().await;
            writer
                .write_all(&data)
                .await
                .map_err(|error| Error::unavailable(format!("writing the exec: {error}")))?;
            writer
                .flush()
                .await
                .map_err(|error| Error::unavailable(format!("writing the exec: {error}")))
        })?;
        Ok(json!(true))
    }

    fn exec_resize(&self, input: &Value) -> Result<Value, Error> {
        let id = text(input, "session")?;
        let exec = {
            let sessions = self.sessions.lock().expect("sessions lock");
            sessions
                .get(id)
                .map(|session| session.exec.clone())
                .ok_or_else(|| Error::not_found(format!("no exec session `{id}`")))?
        };
        let cols = input
            .get("cols")
            .and_then(Value::as_u64)
            .unwrap_or(80)
            .clamp(1, 500) as u32;
        let rows = input
            .get("rows")
            .and_then(Value::as_u64)
            .unwrap_or(24)
            .clamp(1, 500) as u32;
        self.handle
            .block_on(self.runtime.exec_resize(&exec, cols, rows))?;
        Ok(json!(true))
    }

    fn exec_close(&self, input: &Value) -> Result<Value, Error> {
        let id = text(input, "session")?;
        let session = self.sessions.lock().expect("sessions lock").remove(id);
        if let Some(session) = session {
            session.task.abort();
        }
        Ok(json!(true))
    }
}

impl Handler for Service {
    fn call(&self, interface: &str, function: &str, input: Value) -> Result<Value, Error> {
        if interface != RUNTIME {
            return Err(Error::not_found(format!(
                "`{interface}` is not served by the container manager"
            )));
        }
        match function {
            "list-containers" => {
                let value = self.handle.block_on(self.runtime.containers())?;
                Ok(Value::Array(model::containers(&value)))
            }
            "inspect" => {
                let id = text(&input, "id")?;
                Ok(self
                    .handle
                    .block_on(self.runtime.inspect(id))?
                    .map(|value| model::container_from_inspect(&value))
                    .unwrap_or(Value::Null))
            }
            "create" => {
                let request: model::CreateRequest = serde_json::from_value(input)
                    .map_err(|error| Error::invalid(format!("create: {error}")))?;
                let (name, body) = request.body()?;
                let id = self
                    .handle
                    .block_on(self.runtime.create(name.as_deref(), &body))?;
                self.audit(
                    "containers.create",
                    json!({"id": id, "image": request.image, "name": name}),
                );
                let inspected = self.handle.block_on(self.runtime.inspect(&id))?;
                Ok(inspected
                    .map(|value| model::container_from_inspect(&value))
                    .unwrap_or(Value::Null))
            }
            "start" => self.lifecycle(&input, "start"),
            "stop" => self.lifecycle(&input, "stop"),
            "restart" => self.lifecycle(&input, "restart"),
            "pause" => self.lifecycle(&input, "pause"),
            "unpause" => self.lifecycle(&input, "unpause"),
            "kill" => {
                let id = text(&input, "id")?;
                let signal = text(&input, "signal")?;
                if !allowed_signal(signal) {
                    return Err(Error::invalid(format!(
                        "`{signal}` is not an allowlisted signal"
                    )));
                }
                self.handle.block_on(self.runtime.kill(id, signal))?;
                self.audit("containers.kill", json!({"id": id, "signal": signal}));
                Ok(json!(true))
            }
            "remove" => {
                let id = text(&input, "id")?;
                let force = flag(&input, "force");
                self.handle.block_on(self.runtime.remove(id, force))?;
                self.audit("containers.remove", json!({"id": id, "force": force}));
                Ok(json!(true))
            }
            "exec" => self.exec(&input),
            "exec-write" => self.exec_write(&input),
            "exec-resize" => self.exec_resize(&input),
            "exec-close" => self.exec_close(&input),
            "list-images" => {
                let value = self.handle.block_on(self.runtime.images())?;
                Ok(Value::Array(model::images(&value)))
            }
            "remove-image" => {
                let id = text(&input, "id")?;
                let force = flag(&input, "force");
                self.handle.block_on(self.runtime.remove_image(id, force))?;
                self.audit("containers.remove-image", json!({"id": id, "force": force}));
                Ok(json!(true))
            }
            "list-volumes" => {
                let value = self.handle.block_on(self.runtime.volumes())?;
                Ok(Value::Array(model::volumes(&value)))
            }
            "create-volume" => {
                let name = text(&input, "name")?;
                if !valid_volume(name) {
                    return Err(Error::invalid(format!("`{name}` is not a volume name")));
                }
                let value = self.handle.block_on(self.runtime.create_volume(name))?;
                self.audit("containers.create-volume", json!({"name": name}));
                Ok(model::volumes(&json!({"Volumes": [value]}))
                    .into_iter()
                    .next()
                    .unwrap_or(Value::Null))
            }
            "remove-volume" => {
                let name = text(&input, "name")?;
                let force = flag(&input, "force");
                self.handle
                    .block_on(self.runtime.remove_volume(name, force))?;
                self.audit(
                    "containers.remove-volume",
                    json!({"name": name, "force": force}),
                );
                Ok(json!(true))
            }
            "list-networks" => {
                let value = self.handle.block_on(self.runtime.networks())?;
                Ok(Value::Array(model::networks(&value)))
            }
            other => Err(Error::not_found(format!(
                "`{RUNTIME}` has no function `{other}`"
            ))),
        }
    }

    fn stream(&self, interface: &str, function: &str, input: Value) -> Result<ChunkStream, Error> {
        if interface != RUNTIME {
            return Err(Error::not_found(format!(
                "`{interface}` is not served by the container manager"
            )));
        }
        match function {
            "logs" => {
                let id = text(&input, "id")?;
                let tail = input
                    .get("tail")
                    .and_then(|value| value.as_u64().or_else(|| value.as_str()?.parse().ok()))
                    .unwrap_or(200)
                    .min(10_000) as u32;
                let follow = flag(&input, "follow");
                let tty = self
                    .handle
                    .block_on(self.runtime.inspect(id))?
                    .and_then(|value| value.pointer("/Config/Tty").and_then(Value::as_bool))
                    .unwrap_or(false);
                Ok(self
                    .handle
                    .block_on(self.runtime.logs(id, tail, follow, tty))?)
            }
            "stats" => {
                let id = text(&input, "id")?;
                Ok(self.handle.block_on(self.runtime.stats(id))?)
            }
            "pull" => {
                let reference = text(&input, "reference")?;
                self.audit("containers.pull", json!({"reference": reference}));
                Ok(self.handle.block_on(self.runtime.pull(reference))?)
            }
            "exec-attach" => {
                let id = text(&input, "session")?;
                let mut sessions = self.sessions.lock().expect("sessions lock");
                let session = sessions
                    .get_mut(id)
                    .ok_or_else(|| Error::not_found(format!("no exec session `{id}`")))?;
                let receiver = session
                    .receiver
                    .take()
                    .ok_or_else(|| Error::conflict("the exec session is already attached"))?;
                let stream = futures_util::stream::unfold(receiver, |mut receiver| async move {
                    receiver.recv().await.map(|chunk| (Ok(chunk), receiver))
                });
                Ok(Box::pin(stream))
            }
            other => Err(Error::not_found(format!(
                "`{RUNTIME}#{other}` is not a stream of the container manager"
            ))),
        }
    }
}

fn allowed_signal(signal: &str) -> bool {
    matches!(
        signal.trim().to_ascii_uppercase().as_str(),
        "SIGHUP"
            | "SIGINT"
            | "SIGTERM"
            | "SIGKILL"
            | "SIGQUIT"
            | "SIGUSR1"
            | "SIGUSR2"
            | "SIGSTOP"
            | "SIGCONT"
    )
}

fn valid_volume(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "_.-".contains(character))
}

fn text<'a>(input: &'a Value, key: &str) -> Result<&'a str, Error> {
    input
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::invalid(format!("the call needs `{key}`")))
}

fn flag(input: &Value, key: &str) -> bool {
    match input.get(key) {
        Some(Value::Bool(value)) => *value,
        Some(Value::String(value)) => matches!(value.as_str(), "1" | "true" | "yes"),
        Some(Value::Number(value)) => value.as_u64().unwrap_or(0) != 0,
        _ => false,
    }
}

fn prepare_data(data: &std::path::Path) -> Result<(), Error> {
    std::fs::create_dir_all(data)
        .map_err(|error| Error::unavailable(format!("creating {}: {error}", data.display())))?;
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(data, std::fs::Permissions::from_mode(0o700));
    Ok(())
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("containers-service: {error}");
        std::process::exit(if error.kind() == anth_contract::ErrorKind::Invalid {
            2
        } else {
            1
        });
    }
}

async fn run() -> Result<(), Error> {
    let socket = anth_service::socket_argument()?;
    let data = PathBuf::from(argument("--data")?);
    let runtime_socket = PathBuf::from(argument("--runtime")?);
    let version = std::env::args()
        .skip(1)
        .collect::<Vec<_>>()
        .windows(2)
        .find(|pair| pair[0] == "--runtime-version")
        .map(|pair| pair[1].clone())
        .unwrap_or_else(|| "v1.43".to_string());
    let broker = std::env::args()
        .skip(1)
        .collect::<Vec<_>>()
        .windows(2)
        .find(|pair| pair[0] == "--broker")
        .map(|pair| PathBuf::from(&pair[1]));
    prepare_data(&data)?;
    let runtime = Runtime::new(runtime_socket, version);
    let service = Arc::new(Service {
        runtime,
        broker,
        handle: tokio::runtime::Handle::current(),
        sessions: Mutex::new(HashMap::new()),
        counter: AtomicU64::new(0),
    });
    anth_service::serve(&socket, service).await
}

fn argument(flag: &str) -> Result<String, Error> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    for pair in args.windows(2) {
        if pair[0] == flag {
            return Ok(pair[1].clone());
        }
    }
    Err(Error::invalid(format!("`{flag} <value>` is required")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_allowlisted_signals_pass() {
        assert!(allowed_signal("SIGTERM"));
        assert!(allowed_signal("sigkill"));
        assert!(!allowed_signal("SIGSYS"));
        assert!(!allowed_signal("9; rm"));
    }

    #[test]
    fn a_volume_name_cannot_be_a_path() {
        assert!(valid_volume("site"));
        assert!(!valid_volume("../site"));
        assert!(!valid_volume("/var/lib"));
        assert!(!valid_volume(""));
    }
}
