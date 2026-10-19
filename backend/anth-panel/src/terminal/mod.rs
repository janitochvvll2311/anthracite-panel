//! Web terminal: a PTY per session, multiplexed over the line protocol.
//!
//! The pieces are deliberately separate: `session` owns the PTY, `registry` owns
//! the lifecycle and the cap, `auth` owns the token, and the HTTP/WebSocket
//! layer (a later step) owns the browser. Nothing here knows about HTTP.

pub mod auth;
pub mod registry;
pub mod session;
pub mod web;

pub use auth::{Verdict, check, constant_time_eq};
pub use registry::{OpenError, Output, Sessions};
pub use session::{PtySession, Session, SessionResult};
