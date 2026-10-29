//! Per-invocation state and the two imports a guest may reach.
//!
//! The [`Host`] is created with the store and dropped with it, so one call
//! cannot see another's counters or refusals. It holds the manifest, which is
//! what makes `anth_exec` read the *grant*: a guest names a capability id, and
//! every argv element comes from the frozen template plus values validated by
//! the host.

use std::time::Instant;

use anth_wire::{ErrorKind, WireError};
use wasmi::{Caller, Extern, Instance, Memory, Store, Val};

use super::{DEADLINE, trap_kind};
use crate::log::{Level, Logger};
use anth_extension::manifest::{Capability, ExtensionManifest as Manifest};

/// Capabilities one invocation may run.
pub const MAX_CAPABILITY_CALLS: u32 = 32;

/// Bytes one string crossing the boundary may carry.
pub const MAX_STRING_BYTES: usize = 64 * 1024;

/// Bytes of log one invocation may emit.
pub const MAX_LOG_BYTES: usize = 64 * 1024;

/// What a guest answered.
#[derive(Debug)]
pub struct Reply {
    /// The guest's status, as an HTTP code. `__ANTH_EXTENSION_ID__::Status` maps its
    /// variants to exactly these, so a extension cannot invent a code the host does
    /// not already classify.
    pub status: u16,
    pub value: serde_json::Value,
}

impl Reply {
    /// Reads the envelope `__ANTH_EXTENSION_ID__::Response::to_envelope` produces.
    ///
    /// The key is `body`, not `value`, and a `null` reply omits it entirely
    /// rather than sending `"body": null`. Both are the guest's contract, so the
    /// host reads them here and nowhere else.
    ///
    /// A body that is not that envelope becomes a `500` carrying what the guest
    /// did say: the host cannot pretend to understand an answer it could not
    /// parse, and a silent success would hide a broken extension.
    fn decode(text: &str) -> Self {
        #[derive(serde::Deserialize)]
        struct Wire {
            status: u16,
            #[serde(default)]
            body: serde_json::Value,
        }

        match serde_json::from_str::<Wire>(text) {
            Ok(wire) => Self {
                status: wire.status,
                value: wire.body,
            },
            Err(_) => Self {
                status: 500,
                value: serde_json::json!({
                    "error": format!("the extension answered with something that is not a reply: {text}")
                }),
            },
        }
    }

    /// The shared error kind for a non-2xx status, or `None` on success.
    #[allow(dead_code)]
    pub fn error_kind(&self) -> Option<ErrorKind> {
        match self.status {
            200..=299 => None,
            400 => Some(ErrorKind::Invalid),
            403 => Some(ErrorKind::Forbidden),
            404 => Some(ErrorKind::NotFound),
            409 => Some(ErrorKind::Conflict),
            413 => Some(ErrorKind::TooLarge),
            501 => Some(ErrorKind::Unsupported),
            _ => Some(ErrorKind::Internal),
        }
    }
}

/// The store's data: everything the imports may touch.
pub(crate) struct Host {
    capabilities: Vec<Capability>,
    /// Where `anth_log` lines go, and at what level. The extension id is added to
    /// the target so a refusal names the extension that asked for it.
    log: std::sync::Arc<Logger>,
    id: String,
    calls: u32,
    log_bytes: usize,
    started: Instant,
    /// Set when a capability was refused, so the guest's `anth_exec` can return
    /// a sentinel and the host can report the real reason afterwards. The guest
    /// sees only a `0` pointer, which is what `__ANTH_EXTENSION_ID__` documents.
    pub(crate) refusal: Option<String>,
}

impl wasmi::ResourceLimiter for Host {
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> Result<bool, wasmi_core::LimiterError> {
        Ok(desired <= super::MEMORY_LIMIT)
    }

    fn table_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> Result<bool, wasmi_core::LimiterError> {
        Ok(desired <= 10_000)
    }

    /// One instance, one table, one memory: a guest that needs a second of any
    /// of these is not a shape this host agrees to run.
    fn instances(&self) -> usize {
        1
    }

    fn tables(&self) -> usize {
        1
    }

    fn memories(&self) -> usize {
        1
    }
}

impl Host {
    pub(crate) fn new(manifest: &Manifest, log: std::sync::Arc<Logger>) -> Self {
        Self {
            capabilities: manifest.capability.clone(),
            log,
            id: manifest.extension.id.clone(),
            calls: 0,
            log_bytes: 0,
            started: Instant::now(),
            refusal: None,
        }
    }
}

/// Checks an invocation's budget, then runs one capability.
///
/// The deadline is enforced here rather than around the whole call because this
/// is the only point at which control returns to the host. A guest spinning in
/// pure wasm never reaches here, which is exactly what fuel covers; a guest
/// looping over capabilities does reach here, and is stopped by the deadline or
/// the call budget.
pub(crate) fn exec(
    caller: &mut Caller<'_, Host>,
    cap_ptr: i32,
    cap_len: i32,
    args_ptr: i32,
    args_len: i32,
) -> i64 {
    // The capability arrives JSON-encoded because `__ANTH_EXTENSION_ID__` serializes it
    // with `serde_json::to_vec`, so the raw text is `"host.hostname"` with its
    // quotes. Decoding here is what makes the id compare equal to the manifest.
    let capability = match read_guest_string(caller, cap_ptr, cap_len)
        .map(|text| serde_json::from_str::<String>(&text).unwrap_or(text))
    {
        Ok(id) => id,
        Err(message) => {
            caller.data_mut().refusal = Some(message);
            return 0;
        }
    };
    let args_text = match read_guest_string(caller, args_ptr, args_len) {
        Ok(text) => text,
        Err(message) => {
            caller.data_mut().refusal = Some(message);
            return 0;
        }
    };

    let host = caller.data_mut();
    if host.started.elapsed() > DEADLINE {
        host.refusal = Some(format!(
            "the extension ran past the {} second limit",
            DEADLINE.as_secs()
        ));
        return 0;
    }
    host.calls += 1;
    if host.calls > MAX_CAPABILITY_CALLS {
        host.refusal = Some(format!(
            "the extension asked for more than {MAX_CAPABILITY_CALLS} capabilities in one call"
        ));
        return 0;
    }

    let grant = host
        .capabilities
        .iter()
        .find(|c| c.id == capability)
        .cloned();
    let Some(grant) = grant else {
        host.refusal = Some(format!("the manifest grants no capability `{capability}`"));
        return 0;
    };

    let args = match serde_json::from_str::<serde_json::Value>(&args_text) {
        Ok(value) => value,
        Err(e) => {
            host.refusal = Some(format!("the arguments are not JSON: {e}"));
            return 0;
        }
    };

    match super::grant::run(&grant, &args) {
        Ok(output) => match write_host_string(caller, &output) {
            Ok(packed) => packed,
            Err(message) => {
                caller.data_mut().refusal = Some(message);
                0
            }
        },
        Err(message) => {
            caller.data_mut().refusal = Some(message);
            0
        }
    }
}

/// Appends one line to the daemon log, within a budget.
///
/// The guest names a level; the daemon decides whether to print it.
///
/// A number the host has no name for becomes `info` rather than an error: the
/// line is diagnostic, and a extension that is already confused should not also be
/// silent. `info` specifically, because it is the default the operator sees —
/// mapping garbage to `error` would let a guest raise its own priority.
/// The number a guest sends to the level the daemon filters on.
///
/// These five numbers are `__ANTH_EXTENSION_ID__::LogLevel`'s discriminants, written out
/// here because the two crates do not depend on each other. A test pins the
/// table, since a silent drift would relabel every extension line.
pub(crate) fn level_of(number: i32) -> Level {
    match number {
        0 => Level::Error,
        1 => Level::Warn,
        3 => Level::Debug,
        4 => Level::Trace,
        _ => Level::Info,
    }
}

pub(crate) fn log(caller: &mut Caller<'_, Host>, level: i32, ptr: i32, len: i32) {
    let Ok(message) = read_guest_string(caller, ptr, len) else {
        return;
    };
    let host = caller.data_mut();
    host.log_bytes += message.len();
    if host.log_bytes > MAX_LOG_BYTES {
        return;
    }
    let level = level_of(level);
    host.log.event(
        level,
        "extension",
        message.trim_end(),
        &[("id", host.id.as_str())],
    );
}

/// Runs one entry and reads its reply.
pub(crate) fn run(
    loaded: &super::Loaded,
    entry: &str,
    payload: &str,
    log: std::sync::Arc<Logger>,
    fuel: u64,
) -> Result<Reply, WireError> {
    let mut store = loaded.new_store(log, fuel);
    let instance = loaded.instantiate(&mut store)?;

    let func = instance
        .get_typed_func::<(i32, i32), i64>(&store, entry)
        .map_err(|e| {
            WireError::invalid(format!(
                "extension `{}` has no usable `{entry}`: {e}",
                loaded.id()
            ))
        })?;

    let (ptr, len) = write_payload(&mut store, &instance, payload, loaded.id())?;
    let packed = func
        .call(&mut store, (ptr, len))
        .map_err(|e| trap_kind(loaded.id(), &e))?;

    // A refusal is the reason the guest saw a null pointer, so it outranks the
    // trap the guest may have raised while handling one.
    if let Some(reason) = store.data_mut().refusal.take() {
        return Err(WireError::new(
            ErrorKind::Forbidden,
            "extension_refused",
            reason,
        ));
    }

    let text = read_payload(&store, &instance, packed, loaded.id())?;
    Ok(Reply::decode(&text))
}

fn memory(caller: &mut Caller<'_, Host>) -> Result<Memory, String> {
    caller
        .get_export("memory")
        .and_then(Extern::into_memory)
        .ok_or_else(|| "the extension exports no memory".to_string())
}

fn read_guest_string(caller: &mut Caller<'_, Host>, ptr: i32, len: i32) -> Result<String, String> {
    if ptr < 0 || len < 0 {
        return Err("the extension passed a negative pointer".to_string());
    }
    let len = len as usize;
    if len > MAX_STRING_BYTES {
        return Err(format!(
            "the extension passed {len} bytes, over the {MAX_STRING_BYTES} byte limit"
        ));
    }
    let memory = memory(caller)?;
    let mut bytes = vec![0u8; len];
    memory
        .read(&*caller, ptr as usize, &mut bytes)
        .map_err(|e| format!("reading guest memory: {e}"))?;
    String::from_utf8(bytes).map_err(|e| format!("the extension passed invalid UTF-8: {e}"))
}

/// Reserves guest memory through the guest's own allocator, so the allocator
/// owns the block and can free it.
fn write_host_string(caller: &mut Caller<'_, Host>, text: &str) -> Result<i64, String> {
    let bytes = text.as_bytes();
    let len = i32::try_from(bytes.len()).map_err(|_| "the reply is too large".to_string())?;

    let alloc = caller
        .get_export("anth_alloc")
        .and_then(Extern::into_func)
        .ok_or_else(|| "the extension has no anth_alloc".to_string())?;
    let memory = memory(caller)?;

    let mut results = [Val::I32(0)];
    alloc
        .call(&mut *caller, &[Val::I32(len)], &mut results)
        .map_err(|e| format!("anth_alloc trapped: {e}"))?;
    let Val::I32(ptr) = results[0] else {
        return Err("anth_alloc did not answer a pointer".to_string());
    };
    if ptr <= 0 {
        return Err("the extension could not allocate the reply".to_string());
    }

    memory
        .write(&mut *caller, ptr as usize, bytes)
        .map_err(|e| format!("writing guest memory: {e}"))?;
    Ok((i64::from(ptr) << 32) | i64::from(len))
}

fn write_payload(
    store: &mut Store<Host>,
    instance: &Instance,
    payload: &str,
    id: &str,
) -> Result<(i32, i32), WireError> {
    let alloc = instance
        .get_typed_func::<i32, i32>(&*store, "anth_alloc")
        .map_err(|e| {
            WireError::invalid(format!("extension `{id}`: anth_alloc is unusable: {e}"))
        })?;
    let memory = instance
        .get_export(&*store, "memory")
        .and_then(Extern::into_memory)
        .ok_or_else(|| WireError::invalid(format!("extension `{id}` exports no memory")))?;

    let bytes = payload.as_bytes();
    let len =
        i32::try_from(bytes.len()).map_err(|_| WireError::too_large("the payload is too large"))?;
    // An empty payload is legitimate — a `direct` call with no body, or an empty
    // command line — and the guest's allocator answers 0 for it. Writing nothing
    // is not a failure to allocate.
    if len == 0 {
        return Ok((0, 0));
    }
    let ptr = alloc
        .call(&mut *store, len)
        .map_err(|e| WireError::internal(format!("extension `{id}`: anth_alloc trapped: {e}")))?;
    if ptr <= 0 {
        return Err(WireError::internal(format!(
            "extension `{id}` could not allocate {len} bytes"
        )));
    }
    memory
        .write(&mut *store, ptr as usize, bytes)
        .map_err(|e| WireError::internal(format!("writing the payload: {e}")))?;
    Ok((ptr, len))
}

fn read_payload(
    store: &Store<Host>,
    instance: &Instance,
    packed: i64,
    id: &str,
) -> Result<String, WireError> {
    let ptr = (packed >> 32) as i32;
    let len = (packed & 0xFFFF_FFFF) as i32;
    if ptr <= 0 || len <= 0 {
        return Err(WireError::internal(format!(
            "extension `{id}` answered with no payload"
        )));
    }
    let len = len as usize;
    if len > MAX_STRING_BYTES {
        return Err(WireError::too_large(format!(
            "extension `{id}` answered {len} bytes, over the limit"
        )));
    }
    let memory = instance
        .get_export(store, "memory")
        .and_then(Extern::into_memory)
        .ok_or_else(|| WireError::invalid(format!("extension `{id}` exports no memory")))?;
    let mut bytes = vec![0u8; len];
    memory
        .read(store, ptr as usize, &mut bytes)
        .map_err(|e| WireError::internal(format!("reading the reply: {e}")))?;
    String::from_utf8(bytes)
        .map_err(|e| WireError::internal(format!("the reply is not UTF-8: {e}")))
}
