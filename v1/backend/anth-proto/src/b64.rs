//! Base64 for byte payloads inside the line protocol.
//!
//! Terminal bytes are not UTF-8 and may contain newlines, so they cannot travel
//! raw through a newline-delimited JSON frame. Base64 keeps the framing rule
//! ("`\n` is always a frame boundary") literally true at the cost of a third
//! more bytes, measured at ~500 MiB/s to encode on the reference machine.

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::{Deserialize, Deserializer, Serializer};

/// Encodes terminal bytes for a JSON member.
pub fn encode(bytes: &[u8]) -> String {
    STANDARD.encode(bytes)
}

/// Decodes terminal bytes from a JSON member.
pub fn decode(text: &str) -> Result<Vec<u8>, base64::DecodeError> {
    STANDARD.decode(text)
}

pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&encode(bytes))
}

pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
    let text = String::deserialize(deserializer)?;
    decode(&text).map_err(serde::de::Error::custom)
}
