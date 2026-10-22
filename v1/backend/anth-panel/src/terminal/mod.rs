//! Web terminal: a PTY per session, multiplexed over the line protocol.
//!
//! The pieces are deliberately separate: `session` owns the PTY, `registry` owns
//! the lifecycle and the cap, and the HTTP/WebSocket layer (`web`) owns the
//! browser. Nothing here knows about HTTP authentication; the global middleware
//! protects the route like every other `/api` path.

pub mod registry;
pub mod session;
mod shell;
pub(crate) mod web;

pub use registry::{OpenError, Output, Sessions};
pub use session::{PtySession, Session, SessionResult};
pub use shell::supports_line_editing;
