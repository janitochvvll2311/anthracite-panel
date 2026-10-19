//! Terminal requests carried over the line protocol.
//!
//! A terminal is the one operation that is not request/response: output keeps
//! arriving long after the request that opened it. The session is named by an
//! `id` so several can share one socket, and every request here is idempotent
//! from the client's point of view except `Open`.

use serde::{Deserialize, Serialize};

/// Largest terminal chunk the daemon will put in one frame.
///
/// The client batches its reads up to this size; a larger `Output` is a
/// protocol violation, not something to buffer.
pub const MAX_TERMINAL_CHUNK: usize = 64 * 1024;

/// Largest terminal frame accepted from a client, after base64 decoding.
///
/// Paste is the realistic upper bound: a 64 KiB paste of escape sequences.
pub const MAX_TERMINAL_INPUT: usize = 64 * 1024;

/// Terminal size in character cells.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalSize {
    pub cols: u16,
    pub rows: u16,
}

impl TerminalSize {
    /// Smallest size a terminal can usefully have. Below this, applications
    /// divide by zero or refuse to draw.
    pub const MIN: Self = Self { cols: 1, rows: 1 };

    /// Largest size accepted. Real terminals do not exceed a few hundred
    /// columns; the cap keeps a client from allocating an absurd buffer.
    pub const MAX: Self = Self {
        cols: 1000,
        rows: 1000,
    };

    pub fn is_sane(&self) -> bool {
        self.cols >= Self::MIN.cols
            && self.rows >= Self::MIN.rows
            && self.cols <= Self::MAX.cols
            && self.rows <= Self::MAX.rows
    }
}

/// Size used when a client does not state one.
///
/// Shared by the daemon, the CLI and the browser route so their defaults cannot
/// drift apart.
pub const DEFAULT_TERMINAL_SIZE: TerminalSize = TerminalSize { cols: 80, rows: 24 };

/// Arguments for opening a session. Kept in a struct so the variant stays one
/// line in the tagged envelope and can grow without changing the wire shape.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct TerminalOpenArgs {
    /// Initial size. Omitted means the daemon's default, not "size zero".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<TerminalSize>,
}

/// Reply to `TerminalOpen`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct TerminalOpened {
    pub id: u32,
    /// Size the session actually started with, which may differ from the one
    /// requested when the request left it to the daemon.
    pub size: TerminalSize,
    /// Program that was spawned, for the client to display and to explain an
    /// unexpected shell in a deployment that configured another one.
    pub program: String,
    /// Whether the spawned shell is known to provide interactive line editing
    /// and Tab completion. Old daemons omit it, so it defaults to `false`.
    #[serde(default)]
    pub line_editing: bool,
}
