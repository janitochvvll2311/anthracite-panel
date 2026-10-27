//! Terminal sessions: the daemon's one piece of state that outlives a request.
//!
//! The PTY and its reader live in `session`, ownership/limits/expiry in
//! `registry`, shell resolution in `shell`, and the browser bridge in `web`.
//! Nothing here knows about HTTP authentication; the route and the bridge carry
//! the owner explicitly.

mod registry;
mod session;
mod shell;
pub(crate) mod web;

pub use registry::{Chunk, OpenError, Output, Sessions};
pub use session::{PtySession, Session, SessionResult};
pub use shell::{resolve_shell, supports_line_editing};
