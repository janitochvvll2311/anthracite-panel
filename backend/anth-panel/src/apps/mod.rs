//! App manifest, grant and signature model.
//!
//! The app manager lives in `anth-panel`; this module owns the on-disk format
//! and the authority surface it freezes.

pub mod grants;
pub mod manifest;
pub mod signature;
mod state;

pub use grants::{AppGrant, GrantStore};
pub use manifest::{
    Anchor, AppManifest, AppSection, AppSurface, BIN_MANIFEST_FILE, BinSection, Capability,
    MANIFEST_FILE, MANIFEST_FILES, RuntimeKind, RuntimeSection, UiSlot, WASM_MANIFEST_FILE,
    WEB_MANIFEST_FILE, manifest_message, validate_id,
};
