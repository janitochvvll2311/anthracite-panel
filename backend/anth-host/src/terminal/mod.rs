//! Terminal sessions: PTY, ownership, limits and shell resolution.
//!
//! The browser bridge stays in `anth-panel`; this crate owns the machine side.

mod registry;
mod session;
mod shell;

pub use registry::{Chunk, OpenError, SystemTerminal};
pub use session::{PtySession, Session, SessionResult};
pub use shell::{resolve_shell, supports_line_editing};
