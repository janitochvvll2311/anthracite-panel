//! A pseudo-terminal and the program attached to it.
//!
//! This is the only file in the panel that talks to the kernel's PTY interface,
//! and it is the only reason the crate contains `unsafe`. It is deliberately
//! small: open a pair, put a child on the far end, poll and read the near end.
//! Everything about ownership, limits and lifetimes lives in the parent module,
//! which never sees a descriptor.
//!
//! `posix_spawn` rather than `fork`: the daemon is multi-threaded, and what runs
//! between `fork` and `exec` may only call async-signal-safe functions. A spawn
//! file-actions list expresses the redirection without running any of our code
//! in a forked child.

use std::ffi::{CStr, CString};
use std::time::Duration;

use anth_wire::{ErrorKind, WireError};

use super::MAX_CHUNK;

/// One PTY and the child on it.
pub struct Session {
    master: libc::c_int,
    pid: libc::pid_t,
    /// Set once the child has been waited for, so a second wait is impossible.
    ///
    /// Reaping twice is not a slow path, it is a bug: the second `waitpid` can
    /// collect a different child entirely.
    reaped: bool,
    /// Set by [`Session::close`], so `Drop` does not signal a pid that may
    /// already belong to another process.
    closed: bool,
}

impl Session {
    /// Opens a terminal and starts `program` on it.
    ///
    /// The caller has already checked that `program` is allowed and that the
    /// size is sane; this is the mechanical part.
    pub fn spawn(program: &str, cols: u16, rows: u16) -> Result<Self, WireError> {
        let mut master = -1;
        let mut slave = -1;
        let size = libc::winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };

        // The size is given to `openpty`, so the child's first full-screen
        // program sees the right geometry instead of a default 80x24 followed
        // by a resize it may already have drawn against.
        // The name is written by `openpty` itself. `ptsname_r` is the obvious
        // choice and refuses the slave half on musl, which is the libc this
        // binary links; letting `openpty` fill the buffer has no such gap.
        let mut name = [0 as libc::c_char; 128];
        let rc = unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                name.as_mut_ptr(),
                std::ptr::null(),
                &size,
            )
        };
        if rc != 0 {
            return Err(WireError::internal(format!(
                "cannot open a pseudo-terminal: {}",
                std::io::Error::last_os_error()
            )));
        }

        // Both halves must not leak into the child. Without this the child
        // inherits the *master*, which means the terminal is never fully
        // closed: the daemon can die and the shell keeps running on a pts
        // nobody is reading, orphaned to init.
        set_cloexec(master);
        set_cloexec(slave);

        let name = match CStr::from_bytes_until_nul(unsafe {
            std::slice::from_raw_parts(name.as_ptr().cast::<u8>(), name.len())
        }) {
            Ok(text) => text.to_owned(),
            Err(_) => {
                close_fd(master);
                close_fd(slave);
                return Err(WireError::internal(
                    "the pseudo-terminal has no name".to_string(),
                ));
            }
        };

        let spawned = spawn_on(&name, program, slave);
        // The parent keeps only the master. The slave stays open in the child,
        // and this copy must go or the terminal never reports the child gone.
        close_fd(slave);

        let pid = match spawned {
            Ok(pid) => pid,
            Err(error) => {
                close_fd(master);
                return Err(error);
            }
        };

        Ok(Self {
            master,
            pid,
            reaped: false,
            closed: false,
        })
    }

    /// Reads what is available, waiting at most `wait` for the first byte.
    ///
    /// Polling rather than a blocking read is what makes the wait bounded: a
    /// blocking read on a quiet terminal returns only when the program writes
    /// or exits, which may be never.
    pub fn read(&mut self, wait: Duration) -> Result<Vec<u8>, WireError> {
        let timeout = wait.as_millis().min(i32::MAX as u128) as libc::c_int;
        let mut fd = libc::pollfd {
            fd: self.master,
            events: libc::POLLIN,
            revents: 0,
        };

        let ready = unsafe { libc::poll(&mut fd, 1, timeout) };
        if ready < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                return Ok(Vec::new());
            }
            return Err(WireError::internal(format!(
                "cannot wait on the terminal: {error}"
            )));
        }
        if ready == 0 {
            return Ok(Vec::new());
        }

        let mut buffer = vec![0u8; MAX_CHUNK];
        let n = unsafe { libc::read(self.master, buffer.as_mut_ptr().cast(), buffer.len()) };
        match n {
            // EIO is how a Linux PTY reports that the last slave closed, so it
            // is the normal end of a terminal, not a failure.
            -1 => {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() == Some(libc::EIO) {
                    return Ok(Vec::new());
                }
                Err(WireError::internal(format!(
                    "cannot read from the terminal: {error}"
                )))
            }
            0 => Ok(Vec::new()),
            n => {
                buffer.truncate(n as usize);
                Ok(buffer)
            }
        }
    }

    /// Whether the child has finished.
    ///
    /// Waits without blocking, and records the reaping so a later call cannot
    /// collect a different child.
    pub fn exited(&mut self) -> bool {
        if self.reaped {
            return true;
        }
        let mut status = 0;
        let pid = unsafe { libc::waitpid(self.pid, &mut status, libc::WNOHANG) };
        if pid == self.pid {
            self.reaped = true;
            return true;
        }
        false
    }

    /// Sends bytes to the program.
    pub fn write(&mut self, bytes: &[u8]) -> Result<(), WireError> {
        let mut written = 0;
        while written < bytes.len() {
            let n = unsafe {
                libc::write(
                    self.master,
                    bytes[written..].as_ptr().cast(),
                    bytes.len() - written,
                )
            };
            if n < 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(WireError::internal(format!(
                    "cannot write to the terminal: {error}"
                )));
            }
            written += n as usize;
        }
        Ok(())
    }

    /// Tells the program its window changed size.
    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<(), WireError> {
        let size = libc::winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let rc = unsafe { libc::ioctl(self.master, libc::TIOCSWINSZ, &size) };
        if rc != 0 {
            return Err(WireError::internal(format!(
                "cannot resize the terminal: {}",
                std::io::Error::last_os_error()
            )));
        }
        Ok(())
    }

    /// Ends the child and closes the descriptor.
    ///
    /// `SIGHUP` first, because that is what a terminal closing means to the
    /// program on it, then `SIGKILL` if it has not gone: a close that waited
    /// forever for a stubborn child would hold the session open for good.
    pub fn close(&mut self) -> Result<(), WireError> {
        if self.closed {
            return Ok(());
        }
        unsafe { libc::kill(self.pid, libc::SIGHUP) };
        for _ in 0..20 {
            if self.exited() {
                self.finish();
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        unsafe { libc::kill(self.pid, libc::SIGKILL) };
        unsafe { libc::waitpid(self.pid, std::ptr::null_mut(), 0) };
        self.reaped = true;
        self.finish();
        Ok(())
    }

    /// Closes the descriptor and records that this session is done.
    fn finish(&mut self) {
        close_fd(self.master);
        // A closed descriptor is -1, which `close_fd` ignores, so a second
        // finish cannot close a number the process has since reused.
        self.master = -1;
        self.closed = true;
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // A session dropped without `close` still must not leak the descriptor
        // or leave a child behind: both outlive the handle and neither is
        // visible to the caller that dropped it.
        if !self.closed {
            unsafe { libc::kill(self.pid, libc::SIGHUP) };
            if !self.reaped {
                unsafe { libc::waitpid(self.pid, std::ptr::null_mut(), libc::WNOHANG) };
            }
            self.finish();
        }
    }
}

/// Marks a descriptor close-on-exec, so a spawned child does not inherit it.
fn set_cloexec(fd: libc::c_int) {
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFD);
        if flags >= 0 {
            libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC);
        }
    }
}

fn close_fd(fd: libc::c_int) {
    if fd >= 0 {
        unsafe { libc::close(fd) };
    }
}

/// Starts `program` with the PTY as its three standard streams.
///
/// The child **opens** the slave rather than inheriting it. `posix_spawn` has no
/// `login_tty`, but the same effect comes from a spawn attribute and an open:
/// [`POSIX_SPAWN_SETSID`] makes the child a session leader, and its first open
/// of a terminal with no `O_NOCTTY` makes that terminal its controlling one.
/// That is what lets Ctrl-C reach the program instead of the daemon, and what
/// makes `sh -i` keep job control.
///
/// A device node is also not executable, so passing the slave as the program —
/// which reads as the obvious thing to do — fails with `EACCES`.
fn spawn_on(name: &CString, program: &str, slave: libc::c_int) -> Result<libc::pid_t, WireError> {
    let argv_program = CString::new(program)
        .map_err(|_| WireError::invalid(format!("`{program}` is not a program name")))?;

    let mut actions = unsafe { std::mem::zeroed::<libc::posix_spawn_file_actions_t>() };
    let mut attr = unsafe { std::mem::zeroed::<libc::posix_spawnattr_t>() };

    // Every step below undoes the ones before it before returning, so a failure
    // at any point leaves no half-built spawn state behind.
    unsafe {
        let rc = libc::posix_spawn_file_actions_init(&mut actions);
        if rc != 0 {
            return Err(spawn_error(rc, program));
        }
        let rc = libc::posix_spawnattr_init(&mut attr);
        if rc != 0 {
            libc::posix_spawn_file_actions_destroy(&mut actions);
            return Err(spawn_error(rc, program));
        }

        for fd in 0..=2 {
            let rc = libc::posix_spawn_file_actions_addopen(
                &mut actions,
                fd,
                name.as_ptr(),
                libc::O_RDWR,
                0,
            );
            if rc != 0 {
                libc::posix_spawnattr_destroy(&mut attr);
                libc::posix_spawn_file_actions_destroy(&mut actions);
                return Err(spawn_error(rc, program));
            }
        }

        let rc =
            libc::posix_spawnattr_setflags(&mut attr, libc::POSIX_SPAWN_SETSID as libc::c_short);
        if rc != 0 {
            libc::posix_spawnattr_destroy(&mut attr);
            libc::posix_spawn_file_actions_destroy(&mut actions);
            return Err(spawn_error(rc, program));
        }

        let mut argv = [argv_program.as_ptr().cast_mut(), std::ptr::null_mut()];
        // Kept alive until the call returns; `posix_spawn` borrows both lists.
        let env = environment();
        let mut envp: Vec<*mut libc::c_char> =
            env.iter().map(|entry| entry.as_ptr().cast_mut()).collect();
        envp.push(std::ptr::null_mut());

        let mut pid: libc::pid_t = -1;
        // `posix_spawnp` searches `PATH`, which is why an allowlisted name can
        // be handed over as a name and not turned into a path here.
        let rc = libc::posix_spawnp(
            &mut pid,
            argv_program.as_ptr(),
            &actions,
            &attr,
            argv.as_mut_ptr(),
            envp.as_ptr(),
        );
        libc::posix_spawnattr_destroy(&mut attr);
        libc::posix_spawn_file_actions_destroy(&mut actions);

        if rc != 0 {
            return Err(spawn_error(rc, program));
        }
        let _ = slave;
        Ok(pid)
    }
}

fn spawn_error(code: libc::c_int, program: &str) -> WireError {
    WireError::new(
        ErrorKind::Internal,
        format!(
            "cannot start `{program}`: {}",
            std::io::Error::from_raw_os_error(code)
        ),
    )
}

/// The environment the child receives.
///
/// Minimal on purpose: the daemon's own environment may hold credentials that
/// have no business inside a program a remote caller started. `TERM` and a
/// UTF-8 locale are what a full-screen tool needs in order to run at all.
///
/// Returned owned rather than leaked: the caller keeps them alive across the
/// spawn and drops them after, so a daemon that opens many terminals does not
/// grow its heap by one environment per session.
fn environment() -> Vec<CString> {
    let mut entries = vec![
        cstring("TERM=xterm-256color"),
        cstring("LANG=C.UTF-8"),
        cstring("LC_ALL=C.UTF-8"),
    ];
    for name in ["PATH", "HOME"] {
        if let Ok(value) = std::env::var(name)
            && let Ok(entry) = CString::new(format!("{name}={value}"))
        {
            entries.push(entry);
        }
    }
    entries
}

fn cstring(text: &str) -> CString {
    CString::new(text).expect("a literal without a nul")
}
