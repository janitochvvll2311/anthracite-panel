//! The Anthracite Panel wire contract.
//!
//! Messages are length-prefixed JSON: a four-byte big-endian payload length
//! followed by that many bytes of `serde_json`. The crate is deliberately free
//! of panel code so an alternative client or daemon can implement it.

mod error;
mod firewall;
mod frame;
mod interval;
mod message;
mod network;
mod page;
mod process;
mod terminal;

pub use error::{ErrorKind, WireError};
pub use firewall::{FirewallAction, FirewallProtocol, FirewallRule, FirewallSnapshot};
pub use frame::{MAX_FRAME_LEN, read_frame, read_frame_limited, write_frame, write_frame_limited};
pub use interval::{DEFAULT_INTERVAL, MAX_INTERVAL, MIN_INTERVAL, parse_interval};
pub use message::{
    ArchiveEntry, ArchiveFormat, ArchiveListing, FileChunk, FileEntry, FileKind, FileWritten,
    HostInfo, LoadAverage, MemoryInfo, OsInfo, PROTOCOL_VERSION, Request, Response, TrashEntry,
    TrashListing,
};
pub use network::{
    NetworkAddress, NetworkCreate, NetworkEdit, NetworkInterface, NetworkRoute, NetworkSnapshot,
};
pub use page::{DEFAULT_PAGE_LIMIT, MAX_CURSOR_LEN, MAX_PAGE_LIMIT, Page, PageRequest};
pub use process::{
    MAX_SPAWN_ARG_BYTES, MAX_SPAWN_ARGS, ProcessInfo, ProcessSignal, ProcessSnapshot,
    ProcessStarted,
};
pub use terminal::{
    DEFAULT_TERMINAL_SIZE, MAX_TERMINAL_CHUNK, MAX_TERMINAL_DIMENSION, MAX_TERMINAL_INPUT,
    MAX_TERMINAL_WAIT_MS, MIN_TERMINAL_DIMENSION, TerminalOpened, TerminalOutput, TerminalSize,
};

#[cfg(test)]
mod tests;
