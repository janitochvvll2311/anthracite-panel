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
enum Output {
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

/// A running PTY plus the bookkeeping its handle needs.
struct Live {
    session: Mutex<Box<dyn Session>>,
    reader: tokio::sync::Mutex<mpsc::Receiver<Output>>,
    seq: AtomicU64,
    last_used: Mutex<Instant>,
    ended: AtomicBool,
    exit_code: Mutex<Option<i32>>,
}

impl Live {
    fn session(&self) -> MutexGuard<'_, Box<dyn Session>> {
        lock(&self.session)
    }

    fn exit_code(&self) -> Option<i32> {
        *lock(&self.exit_code)
    }

    fn touch(&self, now: Instant) {
        *lock(&self.last_used) = now;
    }

    fn idle_for(&self, now: Instant) -> Duration {
        now.duration_since(*lock(&self.last_used))
    }

    fn is_live(&self, now: Instant, idle: Duration) -> bool {
        idle.is_zero() || self.idle_for(now) < idle
    }

    /// Marks the session as finished and records the exit code once.
    fn end(&self) {
        self.ended.store(true, Ordering::Relaxed);
        let code = self.session().try_exit().ok().flatten();
        *lock(&self.exit_code) = code;
    }

    fn kill(&self) {
        self.session().kill();
    }

    /// An empty answer that keeps the sequence where it is.
    fn snapshot(&self) -> Chunk {
        Chunk {
            seq: self.seq.load(Ordering::Relaxed),
            bytes: Vec::new(),
            eof: false,
            exit_code: None,
        }
    }

    /// The final answer: nothing more will arrive.
    fn finished(&self) -> Chunk {
        Chunk {
            seq: self.seq.load(Ordering::Relaxed),
            bytes: Vec::new(),
            eof: true,
            exit_code: self.exit_code(),
        }
    }
}

struct Loan {
    owner: String,
    live: Arc<Live>,
}

impl Loan {
    fn kill(self) {
        self.live.kill();
    }
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
        self.open_map().len()
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
        if size.validate().is_err() {
            return Err(OpenError::BadSize);
        }

        self.reap();
        let mut open = self.open_map();
        if open.len() >= self.max {
            return Err(OpenError::AtCapacity);
        }

        let (session, reader) = PtySession::spawn(program, size)
            .map_err(|error| OpenError::Spawn(error.to_string()))?;
        let id = format!("t{:x}", self.next.fetch_add(1, Ordering::Relaxed));
        let (sender, receiver) = mpsc::channel(64);
        spawn_output_task(&id, reader, sender);

        open.insert(
            id.clone(),
            Loan {
                owner: owner.to_string(),
                live: Arc::new(Live {
                    session: Mutex::new(Box::new(session)),
                    reader: tokio::sync::Mutex::new(receiver),
                    seq: AtomicU64::new(0),
                    last_used: Mutex::new((self.now)()),
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
        live.touch((self.now)());

        if live.ended.load(Ordering::Relaxed) {
            return Ok(live.finished());
        }

        let wait = Duration::from_millis(u64::from(wait_ms.min(MAX_TERMINAL_WAIT_MS)));
        let mut reader = live.reader.lock().await;
        if live.ended.load(Ordering::Relaxed) {
            return Ok(live.finished());
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
                live.end();
                Ok(live.finished())
            }
            Ok(Some(Output::Failed)) => {
                live.ended.store(true, Ordering::Relaxed);
                Err(WireError::internal("the terminal reader failed"))
            }
            Err(_) => Ok(live.snapshot()),
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
        live.touch((self.now)());
        live.session().write(bytes).map_err(WireError::internal)
    }

    pub fn resize(&self, owner: &str, id: &str, size: TerminalSize) -> Result<(), WireError> {
        size.validate().map_err(WireError::invalid)?;
        let live = self.live(owner, id)?;
        live.touch((self.now)());
        live.session().resize(size).map_err(WireError::internal)
    }

    /// Ends one session. A handle that is already gone is not an error.
    pub fn close(&self, owner: &str, id: &str) -> Result<(), WireError> {
        let loan = {
            let mut open = self.open_map();
            match open.get(id) {
                Some(loan) if loan.owner == owner => open.remove(id),
                Some(_) => return Err(no_such(id)),
                None => return Ok(()),
            }
        };
        if let Some(loan) = loan {
            loan.kill();
        }
        Ok(())
    }

    /// Closes every session belonging to one authenticated browser session.
    pub fn close_owner(&self, owner: &str) -> usize {
        kill_all(self.take_matching(|loan| loan.owner == owner))
    }

    /// Drops sessions idle longer than the configured limit.
    pub fn reap(&self) -> usize {
        if self.idle.is_zero() {
            return 0;
        }
        let now = (self.now)();
        kill_all(self.take_matching(|loan| !loan.live.is_live(now, self.idle)))
    }

    /// Removes and returns the loans matching `predicate`.
    fn take_matching(&self, predicate: impl Fn(&Loan) -> bool) -> Vec<Loan> {
        let mut open = self.open_map();
        let ids: Vec<String> = open
            .iter()
            .filter(|(_, loan)| predicate(loan))
            .map(|(id, _)| id.clone())
            .collect();
        ids.into_iter().filter_map(|id| open.remove(&id)).collect()
    }

    fn live(&self, owner: &str, id: &str) -> Result<Arc<Live>, WireError> {
        let now = (self.now)();
        let open = self.open_map();
        match open.get(id) {
            Some(loan) if loan.owner == owner && loan.live.is_live(now, self.idle) => {
                Ok(Arc::clone(&loan.live))
            }
            _ => Err(no_such(id)),
        }
    }

    fn open_map(&self) -> MutexGuard<'_, HashMap<String, Loan>> {
        lock(&self.inner)
    }
}

/// A lock that ignores poisoning: a panicked request must not take the daemon
/// down with it.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

fn kill_all(loans: Vec<Loan>) -> usize {
    let count = loans.len();
    for loan in loans {
        loan.kill();
    }
    count
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
