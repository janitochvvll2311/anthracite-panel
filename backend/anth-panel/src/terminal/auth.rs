//! The terminal's token gate.
//!
//! This is a lock, not a user system: one operator, one token, a network you
//! already trust. It exists because a terminal is remote code execution and the
//! panel has no other authentication, not because it replaces one.

/// Outcome of checking a presented token.
#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    /// No token is configured, so the terminal is off.
    Disabled,
    /// A token is configured and was matched.
    Allowed,
    /// A token is configured and the presented one did not match.
    Denied,
}

/// Compares two byte strings without leaking where they first differ.
///
/// A plain `==` on strings stops at the first mismatch, so timing reveals the
/// length of the shared prefix. This walks the full length either way.
pub fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0u8;
    for (a, b) in left.iter().zip(right) {
        difference |= a ^ b;
    }
    difference == 0
}

/// Decides whether a request may open a terminal.
///
/// `configured` is the token from the config file. An empty configured token
/// means the feature is switched off, which is distinct from a wrong token:
/// disabled has to answer "not found" rather than "forbidden" so a deployment
/// that never enabled the terminal does not advertise that it exists.
pub fn check(configured: &str, presented: Option<&str>) -> Verdict {
    if configured.is_empty() {
        return Verdict::Disabled;
    }

    match presented {
        Some(token) if constant_time_eq(configured.as_bytes(), token.as_bytes()) => {
            Verdict::Allowed
        }
        _ => Verdict::Denied,
    }
}
