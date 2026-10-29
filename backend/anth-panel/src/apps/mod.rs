//! App manifest, grant and signature model.
//!
//! The app manager lives in `anth-panel`; this module owns the on-disk format
//! and the authority surface it freezes.

pub mod capability;
pub mod grant_v3;
pub mod grants;
pub mod host_catalog;
pub mod manifest;
pub mod registry;
pub mod schema;
pub mod signature;
mod state;

pub use capability::{ApiRoute, CliCommand, Export, Import, ManifestV3};
pub use grant_v3::{GrantV3, Risk};
pub use grants::{AppGrant, GrantStore};
pub use manifest::{
    Anchor, AppManifest, AppSection, AppSurface, BIN_MANIFEST_FILE, BinSection, Capability,
    MANIFEST_FILE, MANIFEST_FILES, RuntimeKind, RuntimeSection, UiSlot, WASM_MANIFEST_FILE,
    WEB_MANIFEST_FILE, manifest_message, validate_id,
};
pub use registry::{AppState, Graph, build as build_graph};
pub use schema::{Signature, Type};
