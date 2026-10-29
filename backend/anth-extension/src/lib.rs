//! Extension manifest, grant and signature primitives.

pub mod grants;
pub mod manifest;
pub mod signature;
mod state;

pub use grants::{ExtensionGrant, GrantStore};
pub use manifest::{
    Anchor, Capability, ExtensionManifest, ExtensionSection, ExtensionSurface, MANIFEST_FILE,
    MANIFEST_FILES, RuntimeKind, RuntimeSection, SERVICE_MANIFEST_FILE, ServiceSection, UiSlot,
    WASM_MANIFEST_FILE, WEB_MANIFEST_FILE, manifest_message, validate_id,
};
