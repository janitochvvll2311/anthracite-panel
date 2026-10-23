//! Raw mode for the interactive prompt.
//!
//! A cooked terminal buffers a whole line and handles Backspace and arrows
//! itself, which is why the old prompt could only ever see a finished line. The
//! editor needs individual keys, so the terminal is put in raw mode for as long
//! as the prompt runs and restored afterwards.
//!
//! Two rules here are not negotiable:
//!
//! - `ECHO` and `ICANON` go off but `ISIG` stays **on**. Ctrl-C must still
//!   interrupt the process, because a panel that ignores it looks hung. The
//!   editor never sees Ctrl-C as a key, which is why [`Key::Interrupt`] handling
//!   is a fallback rather than the main path.
//! - The original settings are restored by a guard, so an early return, an
//!   `Err` or a panic cannot leave the user's terminal unusable. A terminal
//!   stuck in raw mode is worse than a prompt without history.
//!
//! [`Key::Interrupt`]: super::editor::Key::Interrupt

use std::io::{self, IsTerminal};

/// The terminal's saved settings, restored when the value is dropped.
pub struct RawMode {
    /// The settings that were in place before, or `None` when stdin is not a
    /// terminal and nothing had to change.
    pub(crate) original: Option<libc::termios>,
}

impl RawMode {
    /// Puts stdin in raw mode, or reports that there is nothing to do.
    ///
    /// Piped input — `echo help | anth-panel -i` — is not a terminal, and
    /// `tcsetattr` on a pipe fails. That case is normal, not an error: the line
    /// reader still works, it just has no editing to do.
    pub fn enable() -> io::Result<Self> {
        Self::enable_if(io::stdin().is_terminal())
    }

    /// [`enable`](Self::enable) with the terminal decision passed in.
    ///
    /// The decision is a parameter so a test can cover the pipe path without
    /// depending on whether the harness happened to give it a terminal, and
    /// without switching the real one to raw mode by accident.
    pub(crate) fn enable_if(interactive: bool) -> io::Result<Self> {
        if !interactive {
            return Ok(Self { original: None });
        }

        // SAFETY: `tcgetattr` writes into a `termios` it owns; the fd is stdin,
        // which is open for the life of the process.
        let mut original: libc::termios = unsafe { std::mem::zeroed() };
        if unsafe { libc::tcgetattr(libc::STDIN_FILENO, &mut original) } != 0 {
            return Err(io::Error::last_os_error());
        }

        let mut raw = original;

        // Byte-at-a-time input, no echo: the editor echoes.
        raw.c_lflag &= !(libc::ICANON | libc::ECHO | libc::ECHOE | libc::ECHOK | libc::ECHONL);
        // Reading hands over whatever has arrived instead of waiting for a line.
        raw.c_cc[libc::VMIN] = 1;
        raw.c_cc[libc::VTIME] = 0;
        // Keep output post-processing: `\n` must still move to column 0.
        raw.c_oflag |= libc::OPOST;

        // SAFETY: `raw` is a fully initialised `termios` for stdin.
        if unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSADRAIN, &raw) } != 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(Self {
            original: Some(original),
        })
    }

    /// Whether the prompt is on a terminal at all.
    ///
    /// The caller uses this to decide between the editor and plain line
    /// reading: repainting a pipe would insert escape bytes into a log.
    pub fn is_interactive() -> bool {
        io::stdin().is_terminal()
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        let Some(original) = self.original.take() else {
            return;
        };
        // SAFETY: the value was read from stdin earlier in this same process.
        unsafe {
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSADRAIN, &original);
        }
    }
}
