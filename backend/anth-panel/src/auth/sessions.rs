//! Refresh-token records.
//!
//! A refresh token is opaque; only its SHA-256 digest is stored. Every record
//! names a session (`sid`) and a rotation chain. A consumed token stays until
//! expiry so reuse can be detected and the whole chain dropped.

use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::state;

/// How many live records one operator may hold.
pub const MAX_SESSIONS: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRecord {
    /// The session id carried by access and refresh tokens.
    pub sid: String,
    /// SHA-256 of the opaque refresh token, hex.
    pub digest: String,
    /// One login and its rotations.
    pub chain: String,
    /// Unix seconds after which the record is dead whether used or not.
    pub expires: u64,
    /// Whether a refresh already consumed it.
    pub used: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct File {
    version: u32,
    #[serde(default)]
    session: Vec<SessionRecord>,
}

/// Loads records. A missing file is an empty list; a malformed one is refused.
pub fn load(path: &Path) -> Result<Vec<SessionRecord>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("reading {}: {error}", path.display())),
    };
    let file: File = toml::from_str(&text)
        .map_err(|error| format!("{} is not valid session state: {error}", path.display()))?;
    if file.version != 1 {
        return Err(format!(
            "{} has session version {}, expected 1",
            path.display(),
            file.version
        ));
    }
    Ok(file.session)
}

/// Writes records atomically.
pub fn save(path: &Path, records: &[SessionRecord]) -> Result<(), String> {
    let file = File {
        version: 1,
        session: records.to_vec(),
    };
    let body = toml::to_string_pretty(&file)
        .map_err(|error| format!("serializing session state: {error}"))?;
    let text =
        format!("# Anthracite Panel session state. Machine-owned; do not edit by hand.\n{body}");
    state::write_atomic(path, text.as_bytes(), 0o600)
}

/// Removes expired records and caps the list at [`MAX_SESSIONS`].
pub fn trim(records: &mut Vec<SessionRecord>, now: u64) {
    records.retain(|record| record.expires > now);
    records.sort_by_key(|record| record.expires);
    if records.len() > MAX_SESSIONS {
        let drop = records.len() - MAX_SESSIONS;
        records.drain(0..drop);
    }
}

/// SHA-256 hex of an opaque token.
pub fn digest(token: &str) -> String {
    let bytes = Sha256::digest(token.as_bytes());
    hex(&bytes)
}

/// Hex encoding for digests and ids.
pub fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// Decodes a hex string, or `None`.
pub fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::with_capacity(text.len() / 2);
    let bytes = text.as_bytes();
    for pair in bytes.chunks(2) {
        let high = (pair[0] as char).to_digit(16)?;
        let low = (pair[1] as char).to_digit(16)?;
        out.push(((high << 4) | low) as u8);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(sid: &str, expires: u64, used: bool) -> SessionRecord {
        SessionRecord {
            sid: sid.to_string(),
            digest: digest(sid),
            chain: "chain".to_string(),
            expires,
            used,
        }
    }

    #[test]
    fn trim_drops_expired_and_keeps_the_newest() {
        let mut records: Vec<_> = (0..12)
            .map(|i| record(&i.to_string(), 100 + i, false))
            .collect();
        trim(&mut records, 100);
        assert_eq!(records.len(), MAX_SESSIONS);
        assert_eq!(records.first().unwrap().sid, "2");
        assert_eq!(records.last().unwrap().sid, "11");
    }

    #[test]
    fn hex_round_trips() {
        let bytes = [0u8, 1, 15, 16, 255];
        assert_eq!(unhex(&hex(&bytes)).unwrap(), bytes);
        assert!(unhex("0").is_none());
        assert!(unhex("zz").is_none());
    }

    #[test]
    fn a_digest_is_stable_and_not_the_token() {
        assert_eq!(digest("token"), digest("token"));
        assert_ne!(digest("token"), "token");
    }
}
