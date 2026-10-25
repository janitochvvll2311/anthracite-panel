//! Base64 for byte payloads inside a JSON frame.
//!
//! A byte slice that is not UTF-8 cannot travel as a JSON string, and
//! `serde_json` serializes a bare `Vec<u8>` as an array of numbers — every byte
//! becomes `255,`, which triples the payload at the boundary. Base64 costs a
//! third more instead, and keeps the field a string.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Deserializer, Serializer};

/// Serializes bytes as one base64 string.
pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&STANDARD.encode(bytes))
}

/// Reads bytes back from one base64 string.
pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
    let text = String::deserialize(deserializer)?;
    STANDARD
        .decode(&text)
        .map_err(|e| serde::de::Error::custom(format!("invalid base64: {e}")))
}
