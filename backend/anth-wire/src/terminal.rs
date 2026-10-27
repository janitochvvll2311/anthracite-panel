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

/// Reply to [`crate::Request::TerminalOpen`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalOpened {
    /// Opaque handle. The client never invents one.
    pub id: String,
    /// Program the daemon actually spawned.
    pub program: String,
    pub cols: u16,
    pub rows: u16,
    /// Whether the shell has native line editing and Tab completion.
    #[serde(default)]
    pub line_editing: bool,
}

/// Answer to [`crate::Request::TerminalRead`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalOutput {
    pub id: String,
    /// Advances only when bytes are returned, so a gap means a lost answer.
    pub seq: u64,
    /// Bytes, base64 encoded.
    pub bytes_base64: String,
    /// The program has exited. An empty output with `eof` is the end.
    pub eof: bool,
    /// Exit code when `eof` is true, when the platform reports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
}
