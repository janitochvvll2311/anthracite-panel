//! Running a wasm plugin: the manifest, the bundle and the sandbox.
//!
//! This module is the authority boundary. `anth-plugin` defines the contract a
//! guest links, but it cannot decide what a guest may do: the grant is frozen in
//! the manifest, the argv is built here, and the guest only names a capability.

pub mod install;
pub mod manifest;
pub mod registry;
pub mod runtime;

#[cfg(test)]
mod tests;
