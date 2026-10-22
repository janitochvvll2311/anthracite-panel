//! Shared imports for the `process` submodules.

#![allow(unused_imports)]

pub use std::collections::HashMap;
pub use std::ffi::OsStr;
pub use std::fs;
pub use std::io;
pub use std::os::unix::fs::PermissionsExt;
pub use std::os::unix::process::CommandExt;
pub use std::path::Path;
pub use std::process::{Command, Stdio};
pub use std::time::{SystemTime, UNIX_EPOCH};

pub use anth_proto::{
    MAX_LINE_LEN, ProcessInfo, ProcessSignal, ProcessSnapshot, ProcessSpawnError,
    ProcessSpawnErrorKind, ProcessSpawned, Response,
};

pub(crate) use super::{
    CLOCK_TICKS_FALLBACK, MAX_COMMAND_CHARS, MAX_PROCESSES, MIN_COMMAND_CHARS, ProcessPolicy,
};
