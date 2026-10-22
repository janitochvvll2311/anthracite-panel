//! The messages that travel through a frame.
//!
//! One `Request` and one `Response` enum, and a failure is always
//! `Response::Error(WireError)` rather than a per-feature error type. v0.1.0
//! added a request variant, a response variant and three hand-written match arms
//! for every operation; the dispatch trait in `anth-paneld` exists so a feature
//! answers one method instead.

use serde::{Deserialize, Serialize};

use crate::error::WireError;

/// Bumped when a change would make an older peer misread a message.
pub const PROTOCOL_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Request {
    /// Negotiate the protocol version. Refused if the daemon speaks a different
    /// one, so a mixed-version pair fails at the first message instead of
    /// misparsing a later one.
    Hello { protocol_version: u16 },
    /// One snapshot of the host the daemon runs on.
    HostInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Response {
    /// Answer to [`Request::Hello`], carrying the daemon's version.
    Welcome {
        protocol_version: u16,
    },
    HostInfo(HostInfo),
    Error(WireError),
}

impl Response {
    /// The error this response carries, if it is one.
    pub fn error(&self) -> Option<&WireError> {
        match self {
            Response::Error(e) => Some(e),
            _ => None,
        }
    }

    pub fn into_result(self) -> Result<Self, WireError> {
        match self {
            Response::Error(e) => Err(e),
            other => Ok(other),
        }
    }
}

/// Operating system identification, from `os-release` and the kernel.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct OsInfo {
    pub name: String,
    pub version: String,
    pub kernel: String,
    pub arch: String,
}

/// The three load averages `/proc/loadavg` reports.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct LoadAverage {
    pub one: f64,
    pub five: f64,
    pub fifteen: f64,
}

/// Memory and swap totals in kibibytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MemoryInfo {
    pub total_kib: u64,
    pub available_kib: u64,
    pub swap_total_kib: u64,
    pub swap_free_kib: u64,
}

impl MemoryInfo {
    /// Used memory in kibibytes, never negative.
    pub fn used_kib(&self) -> u64 {
        self.total_kib.saturating_sub(self.available_kib)
    }

    /// Used swap in kibibytes, never negative.
    pub fn swap_used_kib(&self) -> u64 {
        self.swap_total_kib.saturating_sub(self.swap_free_kib)
    }
}

/// A point-in-time description of the host.
///
/// Every field degrades to `unknown` or zero when its source is unreadable;
/// a missing `/proc` entry is not a reason to fail the request.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct HostInfo {
    pub hostname: String,
    pub os: OsInfo,
    pub uptime_seconds: u64,
    pub cpu_model: String,
    pub logical_cores: usize,
    pub load: LoadAverage,
    pub memory: MemoryInfo,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> HostInfo {
        HostInfo {
            hostname: "anth".into(),
            os: OsInfo {
                name: "Alpine Linux".into(),
                version: "3.20".into(),
                kernel: "6.6.0".into(),
                arch: "x86_64".into(),
            },
            uptime_seconds: 3600,
            cpu_model: "Example CPU".into(),
            logical_cores: 4,
            load: LoadAverage {
                one: 0.5,
                five: 0.25,
                fifteen: 0.125,
            },
            memory: MemoryInfo {
                total_kib: 1024,
                available_kib: 512,
                swap_total_kib: 0,
                swap_free_kib: 0,
            },
        }
    }

    #[test]
    fn a_request_round_trips_through_json() {
        let request = Request::HostInfo;
        let json = serde_json::to_string(&request).unwrap();
        assert_eq!(json, r#"{"type":"host_info"}"#);
        assert_eq!(serde_json::from_str::<Request>(&json).unwrap(), request);
    }

    #[test]
    fn hello_carries_the_version() {
        let request = Request::Hello {
            protocol_version: PROTOCOL_VERSION,
        };
        let json = serde_json::to_string(&request).unwrap();
        assert_eq!(json, r#"{"type":"hello","data":{"protocol_version":1}}"#);
        assert_eq!(serde_json::from_str::<Request>(&json).unwrap(), request);
    }

    #[test]
    fn a_snapshot_round_trips_through_json() {
        let response = Response::HostInfo(sample());
        let json = serde_json::to_string(&response).unwrap();
        let back: Response = serde_json::from_str(&json).unwrap();
        assert_eq!(back, response);
    }

    #[test]
    fn an_error_response_is_an_error_and_not_a_snapshot() {
        let response = Response::Error(WireError::forbidden("control is off"));
        assert!(response.error().is_some());
        assert!(response.into_result().is_err());

        let ok = Response::HostInfo(sample());
        assert!(ok.error().is_none());
        assert!(ok.into_result().is_ok());
    }

    #[test]
    fn the_wire_tag_matches_the_variant_name_in_snake_case() {
        // An alternative client depends on this spelling, so pin it.
        let response = Response::Welcome {
            protocol_version: PROTOCOL_VERSION,
        };
        assert_eq!(
            serde_json::to_string(&response).unwrap(),
            r#"{"type":"welcome","data":{"protocol_version":1}}"#
        );
    }

    #[test]
    fn used_memory_and_swap_never_go_negative() {
        let mut memory = sample().memory;
        assert_eq!(memory.used_kib(), 512);

        memory.available_kib = 4096;
        memory.swap_total_kib = 10;
        memory.swap_free_kib = 20;
        assert_eq!(memory.used_kib(), 0);
        assert_eq!(memory.swap_used_kib(), 0);
    }

    #[test]
    fn an_unknown_message_type_is_a_parse_error_not_a_default() {
        let result = serde_json::from_str::<Request>(r#"{"type":"nope"}"#);
        assert!(result.is_err(), "an unknown variant must not deserialize");
    }
}
