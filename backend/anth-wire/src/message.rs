//! The messages that travel through a frame.
//!
//! One `Request` and one `Response` enum, and a failure is always
//! `Response::Error(WireError)` rather than a per-feature error type. v0.1.0
//! added a request variant, a response variant and three hand-written match arms
//! for every operation; the dispatch trait in `anth-panel` exists so a feature
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
    /// Every plugin the daemon found, including the disabled ones and why.
    PluginList,
    /// Run one pipe of one plugin.
    ///
    /// `pipe` names the transport: `cli`, `http` or `direct`. It is a field
    /// rather than a separate request per pipe because the *host* chooses the
    /// entry, and the caller has to say which one it is standing in for.
    PluginCall {
        id: String,
        pipe: PluginPipe,
        /// The payload, already in the shape that pipe carries: a command line
        /// for `cli`, a JSON body for `http`, a `{"action":..,"value":..}` for
        /// `direct`.
        payload: String,
    },
}

/// The transport a plugin call stands in for.
///
/// One variant per ABI entry, so an unknown pipe is a parse failure rather than
/// a string the host has to match at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginPipe {
    /// A command line, as `plugin call` would type it.
    Cli,
    /// An HTTP anchor: the payload is the JSON body.
    Http,
    /// A `Request` in the plugin's own vocabulary, skipping both mappers.
    ///
    /// The default because it is the only pipe that needs no translation: a
    /// caller that does not say which pipe it stands in for has already decided
    /// to speak the plugin's own vocabulary.
    #[default]
    Direct,
}

impl PluginPipe {
    /// The ABI entry this pipe is served by.
    ///
    /// The mapping lives here, next to the enum, so a new variant cannot be
    /// added without naming an entry the host will call.
    pub const fn entry(self) -> &'static str {
        match self {
            PluginPipe::Cli => "anth_cli",
            PluginPipe::Http => "anth_http",
            PluginPipe::Direct => "anth_direct",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Response {
    /// Answer to [`Request::Hello`], carrying the daemon's version.
    Welcome {
        protocol_version: u16,
    },
    HostInfo(HostInfo),
    /// Every plugin the daemon found, sorted by id.
    PluginList {
        plugins: Vec<PluginSummary>,
    },
    /// One plugin's answer, already parsed out of its envelope.
    ///
    /// The status travels as a number rather than as an `ErrorKind` because a
    /// plugin may answer `201` or `204`, which no kind carries. A non-2xx status
    /// is still a successful *call*; it is the plugin's outcome, not a transport
    /// failure.
    PluginReply {
        id: String,
        status: u16,
        value: serde_json::Value,
    },
    Error(WireError),
}

/// One plugin as the daemon sees it.
///
/// `disabled_reason` is present only when the plugin could not be loaded, and it
/// is what makes a broken bundle diagnosable from `plugin list` instead of
/// invisible.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginSummary {
    pub id: String,
    /// The short handle the bundle declared, if any.
    ///
    /// Present so a client can show what to type instead of the full id, and so
    /// `plugin call <alias>` is discoverable rather than folklore. Omitted from
    /// the wire when a plugin declares none, which keeps the common case small.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    pub name: String,
    pub version: String,
    /// The anchors it declares, as `["GET /greeting"]`.
    pub anchors: Vec<String>,
    pub disabled_reason: Option<String>,
}

impl PluginSummary {
    pub fn is_enabled(&self) -> bool {
        self.disabled_reason.is_none()
    }
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
