//! The shared vocabulary of the platform.
//!
//! Types and pure rules only: no I/O, no runtime, no HTTP. If a rule can be a
//! function of values it belongs here; if it needs a file, a socket or a
//! component, it belongs somewhere else.

pub mod audit;
pub mod catalog;
pub mod composition;
pub mod descriptor;
pub mod error;
pub mod grant;
pub mod id;
pub mod key;
pub mod manifest;
pub mod signature;
pub mod view;
pub mod world;

pub use audit::{AuditAction, AuditEntry, Outcome};
pub use catalog::{is_native, risk_of, NativeInterface, Risk, NATIVE};
pub use composition::{Composition, Profile, COMPOSITION_VERSION};
pub use descriptor::{Descriptor, Tier, DESCRIPTOR_VERSION};
pub use error::{Error, ErrorEnvelope, ErrorKind};
pub use grant::{Grant, GRANT_VERSION};
pub use id::{parse_version, AppId, InterfaceId, Namespace, Version};
pub use key::{KeyAlgorithm, TrustedKey, KEY_VERSION};
pub use manifest::{
    ExportTarget, Manifest, ServiceClass, ServiceSection, MANIFEST_VERSION,
    SERVICE_INTENT_MANIFEST_VERSION, SERVICE_MANIFEST_VERSION,
};
pub use view::{
    AppProblem, AppView, CompositionView, DegradedApp, FatalView, GrantState, GrantView,
    IntentMode, KeyView, ServiceView, SurfaceApp, SurfaceCli, SurfaceFabAction, SurfaceLink,
    SurfaceRoute, SurfaceSlot, SurfaceView,
};
pub use world::{World, WorldMismatch};
