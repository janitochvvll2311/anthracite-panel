//! The wasm shim for the WireGuard plugin.
//!
//! Everything that decides behaviour is in [`logic`], which has no `wasm`
//! imports and runs as a normal test on the host. This file is only the ABI:
//! allocator, memory plumbing and the two imported functions. Keeping the split
//! this sharp is what lets the plugin be tested without a wasm runtime, and it
//! means a change to the ABI cannot quietly reach the routing code.

extern crate alloc;

pub mod logic;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use logic::{Executor, handle};
use serde_json::Value;

/// Must match `anth_panel::plugins::runtime::ABI_VERSION` and the manifest.
const ABI_VERSION: i32 = 1;

#[link(wasm_import_module = "env")]
unsafe extern "C" {
    /// Runs one frozen recipe. JSON in, `(ptr << 32) | len` out.
    fn anth_exec(cap_ptr: i32, cap_len: i32, args_ptr: i32, args_len: i32) -> i64;
    /// Writes one line to the daemon log.
    fn anth_log(ptr: i32, len: i32);
}

#[unsafe(no_mangle)]
pub extern "C" fn anth_abi_version() -> i32 {
    ABI_VERSION
}

/// The allocator the host calls to place a request in guest memory.
///
/// A real allocator, not the bump allocator the WAT fixture uses: this guest
/// allocates and drops repeatedly while parsing, and a bump allocator would grow
/// linear memory with every temporary.
#[unsafe(no_mangle)]
pub extern "C" fn anth_alloc(len: i32) -> i32 {
    if len <= 0 {
        return 0;
    }
    let mut buffer = Vec::<u8>::with_capacity(len as usize);
    let ptr = buffer.as_mut_ptr() as i32;
    core::mem::forget(buffer);
    ptr
}

/// The ABI lists a release hook. The host does not call it today, but exporting
/// a stub would leak every response if it ever did, so this honours the contract.
#[unsafe(no_mangle)]
pub extern "C" fn anth_free(ptr: i32, len: i32) {
    if ptr <= 0 || len <= 0 {
        return;
    }
    // SAFETY: the contract in `docs/design/plugins.md` is that the host frees a
    // range it received from `anth_alloc` with the same length.
    unsafe {
        drop(Vec::from_raw_parts(ptr as *mut u8, 0, len as usize));
    }
}

/// Reads the request, answers it and returns `(ptr << 32) | len`.
///
/// `catch_unwind` is deliberately absent: `panic = "abort"` in the release
/// profile turns a panic into a trap, and the host reports that as an invalid
/// module rather than a crash of the daemon.
#[unsafe(no_mangle)]
pub extern "C" fn anth_handle(ptr: i32, len: i32) -> i64 {
    let request = read_string(ptr, len);
    let mut host = Host;
    let response = handle(&mut host, &request);
    write_string(&response)
}

fn read_string(ptr: i32, len: i32) -> String {
    if ptr <= 0 || len <= 0 {
        return String::new();
    }
    // SAFETY: the host writes the request into a buffer it got from
    // `anth_alloc(len)` and passes that same pointer and length back.
    let bytes = unsafe { core::slice::from_raw_parts(ptr as *const u8, len as usize) };
    String::from_utf8_lossy(bytes).into_owned()
}

fn write_string(value: &str) -> i64 {
    let bytes = value.as_bytes();
    let ptr = anth_alloc(bytes.len() as i32);
    // SAFETY: `anth_alloc` just reserved exactly this many bytes and returned a
    // pointer to them; nothing else refers to that range yet.
    unsafe {
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr as *mut u8, bytes.len());
    }
    ((ptr as i64) << 32) | bytes.len() as i64
}

/// [`Executor`] over the imported functions.
struct Host;

impl Executor for Host {
    fn exec(&mut self, capability: &str, args: &Value) -> Result<Value, String> {
        let arguments = args.to_string();
        let packed = unsafe {
            anth_exec(
                capability.as_ptr() as i32,
                capability.len() as i32,
                arguments.as_ptr() as i32,
                arguments.len() as i32,
            )
        };
        let reply = read_packed(packed);
        let parsed: Value =
            serde_json::from_str(&reply).map_err(|error| format!("unreadable reply: {error}"))?;
        if parsed.get("ok").and_then(Value::as_bool) == Some(true) {
            Ok(parsed)
        } else {
            Err(parsed
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("the capability failed")
                .to_string())
        }
    }

    fn log(&mut self, message: &str) {
        unsafe { anth_log(message.as_ptr() as i32, message.len() as i32) }
    }
}

fn read_packed(packed: i64) -> String {
    let ptr = (packed >> 32) as i32;
    let len = packed as u32 as i32;
    read_string(ptr, len)
}
