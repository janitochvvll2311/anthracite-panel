//! The guest SDK (§7.1).
//!
//! An app's own `wit/` declares its world; this crate re-exports the generator
//! and carries the host catalog that the build and the engine inject. Keeping
//! the dependency here is what lets the engine change the generator without
//! every app editing its Cargo.toml.

pub use wit_bindgen;
pub use wit_bindgen::*;
