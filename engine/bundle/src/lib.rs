//! The canonical Anthracite bundle primitives shared by the engine, the
//! signer, the conformance kit and the app CLI.
//!
//! The signed message is a compatibility boundary: a signature produced by
//! `anth-sign` must be the one the engine verifies, so the format lives here
//! once instead of in every tool.

#[cfg(feature = "message")]
pub mod message;
#[cfg(feature = "signature")]
pub mod signature;
#[cfg(feature = "message")]
pub mod tree;
#[cfg(feature = "unpack")]
pub mod unpack;

#[cfg(feature = "message")]
pub use message::{message, wit_message, MESSAGE_PREFIX, WIT_PREFIX};
#[cfg(feature = "signature")]
pub use signature::{
    check_public_key, fingerprint, parse_signature, public_key, sha256_hex, signature_base64,
    verify, verify_bytes, PUBLIC_KEY_BYTES, SIGNATURE_BYTES,
};
#[cfg(feature = "message")]
pub use tree::signed_message;
#[cfg(feature = "unpack")]
pub use unpack::unpack;
