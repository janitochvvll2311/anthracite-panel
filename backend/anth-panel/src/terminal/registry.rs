//! Session registry: who may open a terminal, and how many at once.
//!
//! Each session owns one output task. It is the only thing that reads the PTY,
//! so a blocking read can never stall the socket: it forwards bytes into a
//! channel and the connection loop only ever awaits that channel.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use anth_proto::TerminalSize;
use tokio::sync::{Mutex, mpsc};

use crate::terminal::session::{PtySession, Session};

/// What a session's output task reports.
#[derive(Debug)]
pub enum Output {
    /// Bytes the program wrote.
    Data(Vec<u8>),
    /// The program ended with this exit code.
    Exited(i32),
    /// The terminal failed to read. Treated as an exit by the connection.
    Failed,
}

/// Why a terminal could not be opened.
#[derive(Debug, PartialEq, Eq)]
pub enum OpenError {
    /// The panel is configured with `enabled = false`.
    Disabled,
    /// `max_sessions` is already reached.
    AtCapacity,
    /// The size was outside the sane range.
    BadSize,
    /// The PTY or the program could not be started.
    Spawn(String),
}

impl OpenError {
    pub fn detail(&self) -> String {
        match self {
            Self::Disabled => "terminal is disabled".to_string(),
            Self::AtCapacity => "too many terminal sessions".to_string(),
            Self::BadSize => "terminal size is outside the allowed range".to_string(),
            Self::Spawn(e) => e.clone(),
        }
    }
}

struct Entry {
    session: Box<dyn Session>,
    output: Option<mpsc::Receiver<Output>>,
}

pub struct Sessions {
    inner: Mutex<HashMap<u32, Entry>>,
    next_id: AtomicU32,
    max: usize,
}

impl Sessions {
    pub fn new(max: usize) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(HashMap::new()),
            next_id: AtomicU32::new(1),
            max: max.max(1),
        })
    }

    /// Opens a PTY for `program`, honouring the cap, and starts its output task.
    pub async fn open(
        &self,
        program: &str,
        size: TerminalSize,
    ) -> Result<(u32, TerminalSize, String), OpenError> {
        if !size.is_sane() {
            return Err(OpenError::BadSize);
        }

        // Held across the spawn so two callers cannot both pass the cap check.
        let mut sessions = self.inner.lock().await;
        if sessions.len() >= self.max {
            return Err(OpenError::AtCapacity);
        }

        let (session, reader) =
            PtySession::spawn(program, size).map_err(|e| OpenError::Spawn(e.to_string()))?;
        let program = session.program().to_string();
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);

        let (sender, receiver) = mpsc::channel(64);
        spawn_output_task(id, reader, sender);

        sessions.insert(
            id,
            Entry {
                session: Box::new(session),
                output: Some(receiver),
            },
        );

        Ok((id, size, program))
    }

    /// Runs `body` against one session while holding the registry lock.
    ///
    /// Only short control operations go through here — a write, a resize, an
    /// exit check. Reading is never done here: it would stop input while the
    /// shell sits idle at a prompt.
    pub async fn with<T>(&self, id: u32, body: impl FnOnce(&mut dyn Session) -> T) -> Option<T> {
        let mut sessions = self.inner.lock().await;
        sessions
            .get_mut(&id)
            .map(|entry| body(entry.session.as_mut()))
    }

    /// Writes bytes to a session, if it is still open.
    pub async fn write(&self, id: u32, bytes: &[u8]) -> Option<Result<(), String>> {
        self.with(id, |session| session.write(bytes)).await
    }

    /// Resizes a session, if it is still open.
    pub async fn resize(&self, id: u32, size: TerminalSize) -> Option<Result<(), String>> {
        self.with(id, |session| session.resize(size)).await
    }

    /// Takes the output receiver for `id`, leaving the session in the registry.
    ///
    /// The connection loop merges these into one stream, so an idle terminal
    /// costs a parked receiver rather than a polled future.
    pub async fn take_output(&self, id: u32) -> Option<mpsc::Receiver<Output>> {
        let mut sessions = self.inner.lock().await;
        sessions.get_mut(&id)?.output.take()
    }

    /// Removes and kills a session. Returns whether it existed.
    pub async fn close(&self, id: u32) -> bool {
        let mut sessions = self.inner.lock().await;
        match sessions.remove(&id) {
            Some(mut entry) => {
                entry.session.kill();
                true
            }
            None => false,
        }
    }

    pub async fn len(&self) -> usize {
        self.inner.lock().await.len()
    }

    pub async fn is_empty(&self) -> bool {
        self.len().await == 0
    }
}

/// Moves PTY bytes onto a channel from a blocking thread.
fn spawn_output_task(
    id: u32,
    mut reader: Box<dyn std::io::Read + Send>,
    sender: mpsc::Sender<Output>,
) {
    std::thread::Builder::new()
        .name(format!("anth-pty-{id}"))
        .spawn(move || {
            loop {
                match PtySession::read_chunk(reader.as_mut()) {
                    Ok(Some(bytes)) if !bytes.is_empty() => {
                        // A full queue blocks the reader, which is back pressure
                        // on the program, exactly as a slow terminal behaves.
                        if sender.blocking_send(Output::Data(bytes)).is_err() {
                            return;
                        }
                    }
                    Ok(Some(_)) | Ok(None) => {
                        // The exit code lives on the child, not on the read
                        // half; the connection reports it from the registry.
                        let _ = sender.blocking_send(Output::Exited(0));
                        return;
                    }
                    Err(_) => {
                        let _ = sender.blocking_send(Output::Failed);
                        return;
                    }
                }
            }
        })
        .ok();
}
