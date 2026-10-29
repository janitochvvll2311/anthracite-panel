//! Terminal sizes, limits and the values a terminal answer carries.
//!
//! The daemon owns the PTY; the wire carries an opaque handle and base64
//! chunks. Keeping the bounds here means the CLI, the browser bridge and the
//! daemon all reject the same sizes.

use serde::{Deserialize, Serialize};

/// Terminal size in character cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalSize {
    pub cols: u16,
    pub rows: u16,
}

impl TerminalSize {
    pub const MIN: Self = Self { cols: 1, rows: 1 };
    pub const MAX: Self = Self {
        cols: MAX_TERMINAL_DIMENSION,
        rows: MAX_TERMINAL_DIMENSION,
    };

    pub fn is_sane(&self) -> bool {
        (Self::MIN.cols..=Self::MAX.cols).contains(&self.cols)
            && (Self::MIN.rows..=Self::MAX.rows).contains(&self.rows)
    }

    /// Rejects an out-of-range size with the message every transport uses.
    pub fn validate(&self) -> Result<(), String> {
        if self.is_sane() {
            return Ok(());
        }
        Err(format!(
            "terminal size {}x{} is outside {}x{}..{}x{}",
            self.cols,
            self.rows,
            Self::MIN.cols,
            Self::MIN.rows,
            Self::MAX.cols,
            Self::MAX.rows
        ))
    }
}

/// Size used when a client does not state one.
pub const DEFAULT_TERMINAL_SIZE: TerminalSize = TerminalSize { cols: 80, rows: 24 };

/// Smallest accepted dimension. A zero-sized terminal makes applications
/// divide by zero or refuse to draw.
pub const MIN_TERMINAL_DIMENSION: u16 = 1;

/// Largest accepted dimension. A real terminal stays well below this; the cap
/// bounds both the kernel's window size and the read buffer.
pub const MAX_TERMINAL_DIMENSION: u16 = 500;

/// Largest chunk one read returns.
pub const MAX_TERMINAL_CHUNK: usize = 16 * 1024;

/// Largest input frame after base64 decoding.
pub const MAX_TERMINAL_INPUT: usize = 16 * 1024;

/// Longest a read waits for output.
pub const MAX_TERMINAL_WAIT_MS: u32 = 30_000;

/// One message a client sends on the terminal WebSocket.
///
/// The browser and the CLI share this shape, so the daemon has one terminal
/// bridge instead of one per client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum TerminalClientMessage {
    /// Keystrokes, base64 encoded.
    Input {
        data: String,
    },
    Resize {
        cols: u16,
        rows: u16,
    },
    Ping,
}

/// One message the daemon sends on the terminal WebSocket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum TerminalServerMessage {
    /// The session exists. `shell` is the program the daemon spawned.
    Ready {
        shell: String,
        cols: u16,
        rows: u16,
        line_editing: bool,
    },
    /// Terminal bytes, base64 encoded.
    Output {
        data: String,
    },
    /// The program exited and the session is over.
    Exit {
        code: i32,
    },
    /// The daemon could not open or continue the session.
    Error {
        detail: String,
    },
    Pong,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zero_dimension_is_rejected_with_the_shared_message() {
        let error = TerminalSize { cols: 0, rows: 24 }.validate().unwrap_err();
        assert_eq!(error, "terminal size 0x24 is outside 1x1..500x500");
    }

    #[test]
    fn sizes_at_the_bounds_are_accepted() {
        assert!(TerminalSize::MIN.validate().is_ok());
        assert!(TerminalSize::MAX.validate().is_ok());
        assert!(
            TerminalSize {
                cols: 501,
                rows: 24
            }
            .validate()
            .is_err()
        );
    }
}
