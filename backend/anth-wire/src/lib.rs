//! The Anthracite Panel wire contract.
//!
//! Messages are length-prefixed JSON: a four-byte big-endian payload length
//! followed by that many bytes of `serde_json`. The crate is deliberately free
//! of panel code so an alternative client or daemon can implement it.

mod error;
mod frame;
mod interval;
mod message;
mod page;

pub use error::{ErrorKind, WireError};
pub use frame::{MAX_FRAME_LEN, read_frame, read_frame_limited, write_frame, write_frame_limited};
pub use interval::{DEFAULT_INTERVAL, MAX_INTERVAL, MIN_INTERVAL, parse_interval};
pub use message::{
    FileChunk, FileEntry, FileWritten, HostInfo, LoadAverage, MemoryInfo, OsInfo, PROTOCOL_VERSION,
    Request, Response,
};
pub use page::{DEFAULT_PAGE_LIMIT, MAX_CURSOR_LEN, MAX_PAGE_LIMIT, Page, PageRequest};

#[cfg(test)]
mod tests;
