//! Terminal handles: ownership, limits, sequencing and idle expiry.
//!
//! The PTY and its reader thread are the only stateful thing in the daemon that
//! outlives a request. This module keeps that state behind a handle so a
//! transport still answers one request at a time.
//!
//! Reading waits on a channel fed by a dedicated thread, never on the session
//! lock. A quiet shell therefore does not stop input or a resize, which is the
//! property v0.1's reader task was built for.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use anth_wire::{ErrorKind, MAX_TERMINAL_INPUT, MAX_TERMINAL_WAIT_MS, TerminalSize, WireError};
use tokio::sync::mpsc;

use super::session::{PtySession, Session};

/// What the reader thread reports.
#[derive(Debug)]
pub enum Output {
    Data(Vec<u8>),
    Closed,
    Failed,
}

/// Why a terminal could not be opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenError {
    Disabled,
    AtCapacity,
    BadSize,
    Spawn(String),
}

impl OpenError {
    pub fn into_wire(self) -> WireError {
        match self {
            Self::Disabled => WireError::new(
                ErrorKind::Forbidden,
                "terminal_disabled",
                "the terminal is disabled; set `[terminal] enabled = true`",
            ),
            Self::AtCapacity => WireError::new(
                ErrorKind::Conflict,
                "terminal_capacity",
                "the terminal session limit has been reached",
            ),
            Self::BadSize => WireError::invalid("the terminal size is outside the allowed range"),
            Self::Spawn(detail) => WireError::internal(detail),
        }
    }
}

/// One successful read.
#[derive(Debug)]
pub struct Chunk {
    pub seq: u64,
    pub bytes: Vec<u8>,
    pub eof: bool,
    pub exit_code: Option<i32>,
}

struct Live {
    session: Mutex<Box<dyn Session>>,
    reader: tokio::sync::Mutex<mpsc::Receiver<Output>>,
    seq: AtomicU64,
    last_used: Mutex<Instant>,
    ended: AtomicBool,
    exit_code: Mutex<Option<i32>>,
}

struct Loan {
    owner: String,
    live: Arc<Live>,
}

/// The open terminals, by opaque handle.
pub struct Sessions {
    inner: Mutex<HashMap<String, Loan>>,
    next: AtomicU64,
    max: usize,
    idle: Duration,
    enabled: bool,
    now: Arc<dyn Fn() -> Instant + Send + Sync>,
}

impl Sessions {
    pub fn new(enabled: bool, max: usize, idle: Duration) -> Self {
        Self::with_clock(enabled, max, idle, Arc::new(Instant::now))
    }

    pub fn with_clock(
        enabled: bool,
        max: usize,
        idle: Duration,
        now: Arc<dyn Fn() -> Instant + Send + Sync>,
    ) -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            next: AtomicU64::new(1),
            max: max.max(1),
            idle,
            enabled,
            now,
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Opens a PTY for `program`, honouring the cap and starting its reader.
    pub fn open(
        &self,
        owner: &str,
        program: &str,
        size: TerminalSize,
    ) -> Result<String, OpenError> {
        if !self.enabled {
            return Err(OpenError::Disabled);
        }
        if !size.is_sane() {
            return Err(OpenError::BadSize);
        }

        self.reap();
        let mut open = self.lock();
        if open.len() >= self.max {
            return Err(OpenError::AtCapacity);
        }

        let (session, reader) = PtySession::spawn(program, size)
            .map_err(|error| OpenError::Spawn(error.to_string()))?;
        let id = format!("t{:x}", self.next.fetch_add(1, Ordering::Relaxed));
        let (sender, receiver) = mpsc::channel(64);
        spawn_output_task(&id, reader, sender);

        let now = (self.now)();
        open.insert(
            id.clone(),
            Loan {
                owner: owner.to_string(),
                live: Arc::new(Live {
                    session: Mutex::new(Box::new(session)),
                    reader: tokio::sync::Mutex::new(receiver),
                    seq: AtomicU64::new(0),
                    last_used: Mutex::new(now),
                    ended: AtomicBool::new(false),
                    exit_code: Mutex::new(None),
                }),
            },
        );
        Ok(id)
    }

    /// Waits at most `wait_ms` for output.
    ///
    /// This future is cancellation-safe: nothing is consumed until a chunk is
    /// actually returned, so a transport may race it against input.
    pub async fn read(&self, owner: &str, id: &str, wait_ms: u32) -> Result<Chunk, WireError> {
        let live = self.live(owner, id)?;
        self.touch(&live);

        if live.ended.load(Ordering::Relaxed) {
            return Ok(Chunk {
                seq: live.seq.load(Ordering::Relaxed),
                bytes: Vec::new(),
                eof: true,
                exit_code: *live.exit_code.lock().unwrap_or_else(|e| e.into_inner()),
            });
        }

        let wait = Duration::from_millis(u64::from(wait_ms.min(MAX_TERMINAL_WAIT_MS)));
        let mut reader = live.reader.lock().await;
        if live.ended.load(Ordering::Relaxed) {
            return Ok(self.finished(&live));
        }

        match tokio::time::timeout(wait, reader.recv()).await {
            Ok(Some(Output::Data(bytes))) => {
                let seq = live.seq.fetch_add(1, Ordering::Relaxed) + 1;
                Ok(Chunk {
                    seq,
                    bytes,
                    eof: false,
                    exit_code: None,
                })
            }
            Ok(Some(Output::Closed)) | Ok(None) => {
                live.ended.store(true, Ordering::Relaxed);
                let code = live
                    .session
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .try_exit()
                    .ok()
                    .flatten();
                *live.exit_code.lock().unwrap_or_else(|e| e.into_inner()) = code;
                Ok(self.finished(&live))
            }
            Ok(Some(Output::Failed)) => {
                live.ended.store(true, Ordering::Relaxed);
                Err(WireError::internal("the terminal reader failed"))
            }
            Err(_) => Ok(Chunk {
                seq: live.seq.load(Ordering::Relaxed),
                bytes: Vec::new(),
                eof: false,
                exit_code: None,
            }),
        }
    }

    pub fn write(&self, owner: &str, id: &str, bytes: &[u8]) -> Result<(), WireError> {
        if bytes.len() > MAX_TERMINAL_INPUT {
            return Err(WireError::too_large(format!(
                "terminal input is {} bytes, over the {MAX_TERMINAL_INPUT} byte limit",
                bytes.len()
            )));
        }
        let live = self.live(owner, id)?;
        self.touch(&live);
        live.session
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .write(bytes)
            .map_err(WireError::internal)
    }

    pub fn resize(&self, owner: &str, id: &str, size: TerminalSize) -> Result<(), WireError> {
        if !size.is_sane() {
            return Err(WireError::invalid(
                "the terminal size is outside the allowed range",
            ));
        }
        let live = self.live(owner, id)?;
        self.touch(&live);
        live.session
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .resize(size)
            .map_err(WireError::internal)
    }

    /// Ends one session. A handle that is already gone is not an error.
    pub fn close(&self, owner: &str, id: &str) -> Result<(), WireError> {
        let loan = {
            let mut open = self.lock();
            match open.get(id) {
                Some(loan) if loan.owner == owner => open.remove(id),
                Some(_) => return Err(no_such(id)),
                None => return Ok(()),
            }
        };
        if let Some(loan) = loan {
            loan.live
                .session
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .kill();
        }
        Ok(())
    }

    /// Closes every session belonging to one authenticated browser session.
    pub fn close_owner(&self, owner: &str) -> usize {
        let loans: Vec<Loan> = {
            let mut open = self.lock();
            let ids: Vec<String> = open
                .iter()
                .filter(|(_, loan)| loan.owner == owner)
                .map(|(id, _)| id.clone())
                .collect();
            ids.into_iter().filter_map(|id| open.remove(&id)).collect()
        };
        let count = loans.len();
        for loan in loans {
            loan.live
                .session
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .kill();
        }
        count
    }

    /// Drops sessions idle longer than the configured limit.
    pub fn reap(&self) -> usize {
        if self.idle.is_zero() {
            return 0;
        }
        let now = (self.now)();
        let expired: Vec<Loan> = {
            let mut open = self.lock();
            let ids: Vec<String> = open
                .iter()
                .filter(|(_, loan)| !is_live(loan, now, self.idle))
                .map(|(id, _)| id.clone())
                .collect();
            ids.into_iter().filter_map(|id| open.remove(&id)).collect()
        };
        let count = expired.len();
        for loan in expired {
            loan.live
                .session
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .kill();
        }
        count
    }

    fn finished(&self, live: &Live) -> Chunk {
        Chunk {
            seq: live.seq.load(Ordering::Relaxed),
            bytes: Vec::new(),
            eof: true,
            exit_code: *live.exit_code.lock().unwrap_or_else(|e| e.into_inner()),
        }
    }

    fn touch(&self, live: &Live) {
        *live.last_used.lock().unwrap_or_else(|e| e.into_inner()) = (self.now)();
    }

    fn live(&self, owner: &str, id: &str) -> Result<Arc<Live>, WireError> {
        let now = (self.now)();
        let open = self.lock();
        match open.get(id) {
            Some(loan) if loan.owner == owner && is_live(loan, now, self.idle) => {
                Ok(Arc::clone(&loan.live))
            }
            _ => Err(no_such(id)),
        }
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<String, Loan>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn is_live(loan: &Loan, now: Instant, idle: Duration) -> bool {
    if idle.is_zero() {
        return true;
    }
    now.duration_since(
        *loan
            .live
            .last_used
            .lock()
            .unwrap_or_else(|e| e.into_inner()),
    ) < idle
}

fn no_such(id: &str) -> WireError {
    WireError::not_found(format!("no terminal `{id}`"))
}

/// Moves PTY bytes onto the channel from one blocking thread.
fn spawn_output_task(
    id: &str,
    mut reader: Box<dyn std::io::Read + Send>,
    sender: mpsc::Sender<Output>,
) {
    let name = format!("anth-pty-{id}");
    let _ = std::thread::Builder::new().name(name).spawn(move || {
        loop {
            match PtySession::read_chunk(reader.as_mut()) {
                Ok(Some(bytes)) if !bytes.is_empty() => {
                    if sender.blocking_send(Output::Data(bytes)).is_err() {
                        return;
                    }
                }
                Ok(Some(_)) | Ok(None) => {
                    let _ = sender.blocking_send(Output::Closed);
                    return;
                }
                Err(_) => {
                    let _ = sender.blocking_send(Output::Failed);
                    return;
                }
            }
        }
    });
}
