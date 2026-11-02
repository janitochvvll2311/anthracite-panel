//! The shared vocabulary of the platform.
//!
//! Types and pure rules only: no I/O, no runtime, no HTTP. If a rule can be a
//! function of values it belongs here; if it needs a file, a socket or a
//! component, it belongs somewhere else.

pub mod error;
pub mod id;
pub mod manifest;
pub mod world;

pub use error::{Error, ErrorEnvelope, ErrorKind};
pub use id::{parse_version, AppId, InterfaceId, Namespace, Version};
pub use manifest::{Manifest, MANIFEST_VERSION};
pub use world::World;
