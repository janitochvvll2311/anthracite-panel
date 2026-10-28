//! Host-side runners for extensions.
//!
//! The real and demo implementations share one port, so the extension host can
//! select a complete set without knowing which one it received.

pub mod exec;

pub use exec::{DemoExecRunner, ExecOutput, ExecRunner, ExecSandbox, SystemExecRunner};
