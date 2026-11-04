//! The shared vocabulary of the platform.
//!
//! Types and pure rules only: no I/O, no runtime, no HTTP. If a rule can be a
//! function of values it belongs here; if it needs a file, a socket or a
//! component, it belongs somewhere else.

pub mod audit;
pub mod catalog;
pub mod composition;
pub mod error;
pub mod grant;
pub mod id;
pub mod manifest;
pub mod world;

pub use audit::{AuditAction, AuditEntry, Outcome};
pub use catalog::{is_native, risk_of, NativeInterface, Risk, NATIVE};
pub use composition::{Composition, Profile, COMPOSITION_VERSION};
pub use error::{Error, ErrorEnvelope, ErrorKind};
pub use grant::{Grant, GRANT_VERSION};
pub use id::{parse_version, AppId, InterfaceId, Namespace, Version};
pub use manifest::{Manifest, MANIFEST_VERSION};
pub use world::World;
