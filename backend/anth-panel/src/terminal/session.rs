//! A pseudo-terminal running one program.
//!
//! This module owns the PTY and nothing else: no HTTP, no JSON, no policy about
//! who may open one. [`Session`] is the seam that lets the daemon own the PTY
//! later without touching the transport or the API layer.

use std::io::{Read, Write};
use std::sync::Mutex;

use anth_proto::{MAX_TERMINAL_CHUNK, TerminalSize};
use portable_pty::{Child, ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};

pub type SessionResult<T> = Result<T, String>;

/// The control surface of a running terminal.
///
/// Reading is deliberately *not* here. Output arrives on a blocking file
/// descriptor owned by its own thread, so a slow reader must never hold the lock
/// that writes and resizes need. See [`PtySession::spawn`], which hands the read
/// half back separately.
pub trait Session: Send {
    /// Bytes typed by the user. Written as-is, so control sequences pass through.
    fn write(&mut self, bytes: &[u8]) -> SessionResult<()>;
    /// Update the window size and signal the process group.
    fn resize(&mut self, size: TerminalSize) -> SessionResult<()>;
    /// Exit code if the program has finished, `None` while it still runs.
    fn try_exit(&mut self) -> SessionResult<Option<i32>>;
    /// Kill the process group. Idempotent: called on close and on drop.
    fn kill(&mut self);
    /// Program that was spawned, for the client to display.
    fn program(&self) -> &str;
}

pub struct PtySession {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    program: String,
    killed: bool,
}

impl PtySession {
    /// Spawns `program` on a new PTY.
    ///
    /// The child inherits a minimal environment rather than the panel's: the
    /// daemon's environment can hold credentials that have no business in a
    /// browser-driven shell.
    /// Returns the control half and the read half. The read half blocks, so it
    /// belongs on a dedicated thread.
    pub fn spawn(program: &str, size: TerminalSize) -> SessionResult<(Self, Box<dyn Read + Send>)> {
        if !size.is_sane() {
            return Err(format!(
                "terminal size {}x{} is outside {}x{}..{}x{}",
                size.cols,
                size.rows,
                TerminalSize::MIN.cols,
                TerminalSize::MIN.rows,
                TerminalSize::MAX.cols,
                TerminalSize::MAX.rows
            ));
        }

        let pair = native_pty_system()
            .openpty(PtySize {
                rows: size.rows,
                cols: size.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("failed to open a pty: {e}"))?;

        let mut command = CommandBuilder::new(program);
        command.env_clear();
        command.env("TERM", "xterm-256color");
        if let Ok(path) = std::env::var("PATH") {
            command.env("PATH", path);
        }
        if let Ok(home) = std::env::var("HOME") {
            command.env("HOME", home);
        }

        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|e| format!("failed to spawn {program}: {e}"))?;
        drop(pair.slave);

        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| format!("failed to read from the pty: {e}"))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| format!("failed to write to the pty: {e}"))?;
        let killer = child.clone_killer();

        Ok((
            Self {
                master: pair.master,
                writer,
                child,
                killer: Mutex::new(killer),
                program: program.to_string(),
                killed: false,
            },
            reader,
        ))
    }

    /// Reads one chunk, blocking. Called from the output thread.
    pub fn read_chunk(reader: &mut (dyn Read + Send)) -> SessionResult<Option<Vec<u8>>> {
        let mut buffer = vec![0u8; MAX_TERMINAL_CHUNK];
        match reader.read(&mut buffer) {
            // EIO is how a Linux PTY reports that the last slave closed.
            Err(e) if e.kind() == std::io::ErrorKind::Other => Ok(None),
            Err(e) => Err(format!("failed to read from the pty: {e}")),
            Ok(0) => Ok(None),
            Ok(n) => {
                buffer.truncate(n);
                Ok(Some(buffer))
            }
        }
    }
}

impl Session for PtySession {
    fn write(&mut self, bytes: &[u8]) -> SessionResult<()> {
        self.writer
            .write_all(bytes)
            .map_err(|e| format!("failed to write to the pty: {e}"))
    }

    fn resize(&mut self, size: TerminalSize) -> SessionResult<()> {
        if !size.is_sane() {
            return Err(format!(
                "refusing to resize to {}x{}: outside the sane range",
                size.cols, size.rows
            ));
        }
        self.master
            .resize(PtySize {
                rows: size.rows,
                cols: size.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("failed to resize the pty: {e}"))
    }

    fn try_exit(&mut self) -> SessionResult<Option<i32>> {
        match self.child.try_wait() {
            Ok(Some(status)) => Ok(Some(status.exit_code() as i32)),
            Ok(None) => Ok(None),
            Err(e) => Err(format!("failed to wait for the child: {e}")),
        }
    }

    fn kill(&mut self) {
        if self.killed {
            return;
        }
        self.killed = true;

        // `portable-pty` signals only the shell's pid. The child called
        // `setsid()`, so it leads its own process group and a negated pid
        // reaches everything it started. Without this, `sleep 100 | cat`
        // survives its shell.
        if let Some(pid) = self.child.process_id() {
            let group = -(pid as i32);
            unsafe {
                libc::kill(group, libc::SIGHUP);
                libc::kill(group, libc::SIGKILL);
            }
        }

        if let Ok(mut killer) = self.killer.lock() {
            let _ = killer.kill();
        }

        // Reap it. `portable-pty` never waits, so without this the pid stays a
        // zombie for the life of the daemon. SIGKILL cannot be caught, so this
        // loop ends in one or two iterations.
        for _ in 0..40 {
            match self.child.try_wait() {
                Ok(Some(_)) | Err(_) => return,
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(5)),
            }
        }
    }

    fn program(&self) -> &str {
        &self.program
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        // A client that disappears must not leave a shell behind.
        self.kill();
    }
}
