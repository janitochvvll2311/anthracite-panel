//! Transport for the Anthracite panel wire protocol.
//!
//! Messages are length-prefixed JSON: a 4-byte big-endian payload length
//! followed by that many bytes of `serde_json`. This replaces v0.1.0's
//! newline-delimited framing, whose 1 MiB line cap forced every list feature to
//! grow a truncation pass (see `docs/design/v0.2.0.md`, lesson 1). A frame here
//! is bounded only by [`MAX_FRAME_LEN`], and exceeding it is an error the caller
//! sees — never a silently shortened list.
//!
//! The crate is deliberately free of panel code: an alternative client or
//! daemon must be implementable against this module alone.

mod error;
mod frame;
mod message;

pub use error::{ErrorKind, WireError};
pub use frame::{MAX_FRAME_LEN, read_frame, read_frame_limited, write_frame, write_frame_limited};
pub use message::{HostInfo, LoadAverage, MemoryInfo, OsInfo, PROTOCOL_VERSION, Request, Response};
