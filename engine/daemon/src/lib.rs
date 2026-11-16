//! The engine: configuration, state, the socket and the supervisor.
//!
//! A headless deployment runs the `anth-engine` binary; the official panel links
//! these modules and mounts the router in its own process (§6.5). Keeping the
//! daemon a library with a thin binary is what lets a test drive the supervisor
//! without spawning a process.

pub mod boot;
pub mod config;
pub mod log;
pub mod sandbox;
pub mod service;
pub mod socket;
pub mod state;
pub mod supervisor;
