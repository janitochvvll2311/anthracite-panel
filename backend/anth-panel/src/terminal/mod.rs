//! Terminal sessions: the daemon's one piece of state that outlives a request.
//!
//! Every other request is answered from a value the daemon already has — the
//! host's `/proc`, a file at a cursor, a plugin bundle. A terminal is not: it is
//! a child process, two file descriptors and a buffer, and none of that fits in
//! one request. So this module is the seam where "one request, one response"
//! gives way to a handle.
//!
//! It is split in two so each half is testable alone:
//!
//! - [`Sessions`] owns the handles: who may use one, how long it lives, how many
//!   exist. It never touches a file descriptor itself, so the rules can be
//!   tested against a clock the test controls.
//! - [`Session`] is the platform seam: a PTY and its child.
//!
//! Nothing here talks to a transport. It returns bytes and [`WireError`]s, and
//! `dispatch` turns them into the answers in `anth-wire`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use anth_wire::{ErrorKind, WireError};

mod pty;
pub use pty::Session;

/// The most sessions one daemon holds at once.
///
/// A terminal is a process and a descriptor, so the cap is a resource limit
/// rather than a preference. Reaching it is refused by name instead of evicting
/// someone: a terminal vanishing under a user is worse than a refusal that says
/// why.
pub const MAX_SESSIONS: usize = 8;

/// The longest a session survives with nobody using it.
///
/// A closed browser tab must not leave a shell running until the daemon stops.
/// Any request against the session resets this, so a session in use never
/// expires however quiet the program is.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// The largest terminal, in columns and rows.
///
/// Bounded because the numbers reach the kernel as a window size and also size
/// a read buffer: a caller-supplied `65535` would ask for a buffer that large.
pub const MAX_DIMENSION: u16 = 500;

/// The smallest terminal worth opening.
pub const MIN_DIMENSION: u16 = 1;

/// The most bytes one read returns.
///
/// Sixteen kibibytes is several dense screenfuls and keeps a frame well inside
/// `MAX_FRAME_LEN` after base64 grows it by a third.
pub const MAX_CHUNK: usize = 16 * 1024;

/// The longest a read waits for output.
///
/// The same rule as a file read: whatever the caller asks for, the daemon
/// answers within this, so a client's own timeout only ever means "the daemon
/// is gone".
pub const MAX_WAIT_MS: u32 = 30_000;

/// The programs a session may start.
///
/// An allowlist, not a path check. Starting a program is not reading a file: the
/// question is not "where may this reach" but "what may run at all", and only
/// the first question has a path-shaped answer.
pub const ALLOWED_PROGRAMS: [&str; 2] = ["sh", "bash"];

/// Whether terminals are served at all.
///
/// Off by default and opted into. A terminal is remote execution of an
/// allowlisted program under the daemon's user, a wider authority than
/// inspecting a host, so running a daemon does not imply granting it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct App {
    pub enabled: bool,
}

impl App {
    /// Refuses when terminal support is off, naming the setting to change.
    pub fn refuse_unless_enabled(&self) -> Result<(), WireError> {
        if self.enabled {
            return Ok(());
        }
        Err(WireError::new(
            ErrorKind::Forbidden,
            "terminal support is off; set `[terminal] enabled = true` in config.toml".to_string(),
        ))
    }
}

/// One session's mutable state.
///
/// Behind a lock of its own rather than the registry's: a read blocks until the
/// program writes, and holding one lock for that would stop every other session
/// for its duration.
struct Live {
    session: Session,
    seq: u64,
    last_used: Instant,
    /// Whether the end has already been reported.
    ///
    /// Kept because a client may ask once more after the end, and the second
    /// answer must still be the end rather than a wait that cannot finish.
    ended: bool,
}

struct Loan {
    owner: String,
    live: Arc<Mutex<Live>>,
}

/// What one successful read returned.
#[derive(Debug)]
pub struct Chunk {
    pub seq: u64,
    pub bytes: Vec<u8>,
    pub eof: bool,
}

/// The open terminals, by handle.
///
/// The clock is a parameter rather than `Instant::now()` at each use, so idle
/// expiry is tested without sleeping for fifteen minutes: a test that slept
/// would only prove that sleeping works.
pub struct Sessions {
    inner: Mutex<HashMap<String, Loan>>,
    next: AtomicU64,
    now: Box<dyn Fn() -> Instant + Send + Sync>,
    idle: Duration,
    max: usize,
}

impl Sessions {
    /// A registry with the daemon's real clock and limits.
    pub fn new() -> Self {
        Self::with_clock(Box::new(Instant::now), IDLE_TIMEOUT, MAX_SESSIONS)
    }

    /// A registry whose clock and limits the caller controls.
    pub fn with_clock(
        now: Box<dyn Fn() -> Instant + Send + Sync>,
        idle: Duration,
        max: usize,
    ) -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            next: AtomicU64::new(1),
            now,
            idle,
            max,
        }
    }

    /// How many sessions are held, expired ones not yet reaped included.
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    /// Opens a session, or refuses by name.
    ///
    /// The cap is checked before the child exists: a refusal that had already
    /// started a process would leave it to be killed on the way out.
    pub fn open(
        &self,
        owner: &str,
        program: &str,
        cols: u16,
        rows: u16,
    ) -> Result<String, WireError> {
        if !ALLOWED_PROGRAMS.contains(&program) {
            return Err(WireError::new(
                ErrorKind::Forbidden,
                format!(
                    "`{program}` is not a program this daemon will run; try one of {}",
                    ALLOWED_PROGRAMS.join(", ")
                ),
            ));
        }
        check_size(cols, rows)?;

        let now = (self.now)();
        let mut open = self.lock();
        let expired = self.take_expired(&mut open, now);
        drop(open);
        close_all(expired);

        let mut open = self.lock();
        if open.len() >= self.max {
            return Err(WireError::new(
                ErrorKind::Conflict,
                format!(
                    "{} terminals are already open, which is the limit; close one first",
                    self.max
                ),
            ));
        }

        let session = Session::spawn(program, cols, rows)?;
        let id = format!("t{:x}", self.next.fetch_add(1, Ordering::Relaxed));
        open.insert(
            id.clone(),
            Loan {
                owner: owner.to_string(),
                live: Arc::new(Mutex::new(Live {
                    session,
                    seq: 0,
                    last_used: now,
                    ended: false,
                })),
            },
        );
        Ok(id)
    }

    /// Waits at most `wait_ms` for output, and answers what arrived.
    ///
    /// `seq` advances only when bytes are returned: an empty answer that moved
    /// the counter would make "quiet program" indistinguishable from "a packet
    /// went missing", and a gap is the only way a client could notice a loss.
    pub fn read(&self, owner: &str, id: &str, wait_ms: u32) -> Result<Chunk, WireError> {
        let wait = Duration::from_millis(u64::from(wait_ms.min(MAX_WAIT_MS)));
        let live = self.live(owner, id)?;
        let mut live = live.lock().unwrap_or_else(|e| e.into_inner());
        live.last_used = (self.now)();

        if live.ended {
            return Ok(Chunk {
                seq: live.seq,
                bytes: Vec::new(),
                eof: true,
            });
        }

        let mut bytes = live.session.read(Duration::ZERO)?;
        if bytes.is_empty() && !wait.is_zero() {
            bytes = live.session.read(wait)?;
        }

        let eof = live.session.exited();
        if eof {
            live.ended = true;
        }
        if !bytes.is_empty() {
            live.seq += 1;
        }
        Ok(Chunk {
            seq: live.seq,
            bytes,
            eof,
        })
    }

    /// Writes bytes to the session, as a keystroke sequence.
    pub fn write(&self, owner: &str, id: &str, bytes: &[u8]) -> Result<(), WireError> {
        let live = self.live(owner, id)?;
        let mut live = live.lock().unwrap_or_else(|e| e.into_inner());
        live.last_used = (self.now)();
        live.session.write(bytes)
    }

    /// Tells the program its window changed size.
    pub fn resize(&self, owner: &str, id: &str, cols: u16, rows: u16) -> Result<(), WireError> {
        check_size(cols, rows)?;
        let live = self.live(owner, id)?;
        let mut live = live.lock().unwrap_or_else(|e| e.into_inner());
        live.last_used = (self.now)();
        live.session.resize(cols, rows)
    }

    /// Ends the session and reaps the child.
    ///
    /// Closing a session that is already gone is not an error: the caller asked
    /// for it not to exist, and it does not.
    pub fn close(&self, owner: &str, id: &str) -> Result<(), WireError> {
        let live = self.live(owner, id)?;
        let mut open = self.lock();
        let loan = open.remove(id);
        drop(open);

        // The session lock is still held by `live`, which is what makes a close
        // wait for an in-flight read instead of pulling the descriptor from
        // under it.
        let mut live = live.lock().unwrap_or_else(|e| e.into_inner());
        match loan {
            Some(_) => live.session.close(),
            None => Ok(()),
        }
    }

    /// Drops every session idle longer than the limit, returning how many.
    ///
    /// Called by the daemon on a timer, so an idle daemon still frees its
    /// shells. A test calls it directly with its own clock, so it needs no
    /// timer and no sleep.
    pub fn reap(&self) -> usize {
        let now = (self.now)();
        let expired = {
            let mut open = self.lock();
            self.take_expired(&mut open, now)
        };
        close_all(expired)
    }

    /// Removes and returns the sessions past their idle limit.
    fn take_expired(
        &self,
        open: &mut HashMap<String, Loan>,
        now: Instant,
    ) -> Vec<Arc<Mutex<Live>>> {
        let mut expired = Vec::new();
        open.retain(|_, loan| {
            let keep = is_live(loan, now, self.idle);
            if !keep {
                expired.push(Arc::clone(&loan.live));
            }
            keep
        });
        expired
    }

    /// The shared state of one session, or "no such terminal".
    fn live(&self, owner: &str, id: &str) -> Result<Arc<Mutex<Live>>, WireError> {
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
        // A panic elsewhere must not turn every later request into a poisoning
        // error that says nothing about the terminal.
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Default for Sessions {
    fn default() -> Self {
        Self::new()
    }
}

/// Whether a session is still young enough to use.
fn is_live(loan: &Loan, now: Instant, idle: Duration) -> bool {
    now.duration_since(
        loan.live
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .last_used,
    ) < idle
}

fn close_all(sessions: Vec<Arc<Mutex<Live>>>) -> usize {
    let count = sessions.len();
    for live in sessions {
        let mut live = live.lock().unwrap_or_else(|e| e.into_inner());
        let _ = live.session.close();
    }
    count
}

/// The answer for a handle that is unknown, expired, or somebody else's.
///
/// One answer for all three: a caller that can tell them apart can probe for
/// other sessions' handles.
fn no_such(id: &str) -> WireError {
    WireError::new(ErrorKind::NotFound, format!("no terminal `{id}`"))
}

fn check_size(cols: u16, rows: u16) -> Result<(), WireError> {
    for (name, value) in [("cols", cols), ("rows", rows)] {
        if !(MIN_DIMENSION..=MAX_DIMENSION).contains(&value) {
            return Err(WireError::new(
                ErrorKind::Invalid,
                format!("`{name}` of {value} is outside {MIN_DIMENSION}..={MAX_DIMENSION}"),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
