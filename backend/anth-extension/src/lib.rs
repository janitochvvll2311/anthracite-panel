//! Extension manifest, grant and signature primitives.

pub mod grants;
pub mod manifest;
pub mod signature;
mod state;

pub use grants::{ExtensionGrant, GrantStore};
pub use manifest::{
    Anchor, BIN_MANIFEST_FILE, BinSection, Capability, ExtensionManifest, ExtensionSection,
    ExtensionSurface, MANIFEST_FILE, MANIFEST_FILES, RuntimeKind, RuntimeSection, UiSlot,
    WASM_MANIFEST_FILE, WEB_MANIFEST_FILE, manifest_message, validate_id,
};
