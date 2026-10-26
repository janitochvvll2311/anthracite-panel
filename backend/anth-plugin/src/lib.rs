//! The contract a plugin guest links.
//!
//! Phase 0 keeps only the ABI version and the shape of the crate: `no_std`,
//! no panel dependency, no authority. The three pipes, `Plugin` trait and
//! executor arrive with the plugin phase.

#![no_std]

extern crate alloc;

/// The guest ABI this crate implements. Must match the host and `plugin.toml`.
pub const ABI_VERSION: i32 = 3;
