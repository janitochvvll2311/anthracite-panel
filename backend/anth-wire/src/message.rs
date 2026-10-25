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
    /// One slice of a file the daemon is allowed to read.
    ///
    /// `offset` rather than a handle, because a file is re-readable by
    /// position: the cursor *is* the identity, so the daemon keeps no state
    /// between calls and a client that drops the answer simply asks again.
    /// `len` is bounded by the daemon, not by the caller.
    ///
    /// `wait_ms` is how long the daemon may wait for bytes to arrive at
    /// `offset` before answering an empty slice. Zero — what a caller that
    /// omits it gets — reads what is there and returns at once, which is a
    /// copy. A positive value is how a follower reads a file that grows: the
    /// answer still says `eof`, so "the file ends here" and "nothing arrived in
    /// time" stay different answers even though both can be empty.
    ///
    /// The wait is bounded by the daemon and capped well below a client's own
    /// timeout, so no request can hang: an empty answer after the wait is the
    /// daemon saying it waited and there was nothing, not a request that failed.
    FileRead {
        path: String,
        offset: u64,
        len: u32,
        #[serde(default)]
        wait_ms: u32,
    },
    /// Open a shell on a pseudo-terminal and name it.
    ///
    /// The answer is a handle, which is the one piece of state this protocol
    /// gives the daemon: a terminal is a process plus two file descriptors, and
    /// there is no cursor that can stand in for either. Everything else about
    /// it is a request like any other, so `dispatch` still answers exactly once.
    ///
    /// `cols` and `rows` are the size to start at; `program` is validated
    /// against the daemon's allowlist, not taken as a path to run.
    TerminalOpen {
        program: String,
        cols: u16,
        rows: u16,
    },
    /// One read from an open terminal, waiting at most `wait_ms` for output.
    ///
    /// A terminal is the case where waiting is the *only* way to read it: the
    /// program may have written nothing yet, and an immediate empty answer
    /// cannot be told apart from a quiet program. The daemon caps the wait, so
    /// a caller can always walk away.
    TerminalRead { id: String, wait_ms: u32 },
    /// Send bytes to the terminal, as a keystroke sequence.
    TerminalWrite {
        id: String,
        #[serde(with = "crate::b64")]
        bytes: Vec<u8>,
    },
    /// Tell the program its window changed size.
    TerminalResize { id: String, cols: u16, rows: u16 },
    /// End the session and reap the child.
    TerminalClose { id: String },
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
    /// One slice of a file, answering [`Request::FileRead`].
    ///
    /// `offset` is where this slice started and `next` is where the following
    /// one does, so a client never has to assume the daemon filled the request.
    /// `eof` is explicit: an empty slice at the end and an empty slice before
    /// the end are different answers, and only this flag tells them apart.
    FileSlice {
        path: String,
        offset: u64,
        #[serde(with = "crate::b64")]
        bytes: Vec<u8>,
        next: u64,
        eof: bool,
        /// Whether the daemon waited for bytes that did not arrive.
        ///
        /// Defaults to `false` so a peer that predates the wait reads an answer
        /// about bytes rather than one about time.
        #[serde(default)]
        waited: bool,
    },
    /// A terminal now exists under `id`.
    ///
    /// `program` is echoed back so a client can label the session with what it
    /// actually got, rather than with what it asked for.
    TerminalOpened {
        id: String,
        program: String,
        cols: u16,
        rows: u16,
    },
    /// Output from a terminal, answering [`Request::TerminalRead`].
    ///
    /// `seq` starts at zero per session and increases by one per non-empty
    /// answer, so a client can notice a gap instead of quietly splicing two
    /// unrelated moments together. `eof` says the program has exited: an empty
    /// read with `eof` is the end, and without it means the wait expired with
    /// nothing written.
    TerminalOutput {
        id: String,
        seq: u64,
        #[serde(with = "crate::b64")]
        bytes: Vec<u8>,
        eof: bool,
    },
    /// The request had no answer of its own, and succeeded.
    ///
    /// A write, a resize and a close all mean "done", and none of them has a
    /// value to return. One variant rather than three identical ones: what a
    /// client needs to know is that the request was accepted.
    Accepted,
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
    /// The places the bundle asks to be mounted, and the module to mount there.
    ///
    /// Empty for a disabled plugin: the manifest did not survive validation, so
    /// it is not a claim the daemon repeats as if it had been checked. The
    /// anchors are kept for the opposite reason — they say why the plugin was
    /// wanted in the first place.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ui: Vec<UiSlotSummary>,
    pub disabled_reason: Option<String>,
}

/// One mount point a bundle declares.
///
/// A slot is a name, not a URL: `dashboard.widget` says *where in the shell*,
/// and the shell decides what that means. The module is a path inside the
/// bundle, served by the daemon at `/p/<id>/<module>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiSlotSummary {
    pub slot: String,
    pub tag: String,
    pub module: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
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
