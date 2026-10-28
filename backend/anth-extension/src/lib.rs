//! Extension manifest, grant and signature primitives.

pub mod grants;
pub mod manifest;
pub mod signature;
mod state;

pub use grants::{ExtensionGrant, GrantStore};
pub use manifest::{
    Anchor, Capability, ExtensionManifest, ExtensionSection, ExtensionSurface, RuntimeKind,
    RuntimeSection, UiSlot, validate_id,
};
