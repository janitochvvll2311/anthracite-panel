//! The wasm shim for the hello plugin.
//!
//! Everything that decides behaviour is in [`logic`], which has no `wasm`
//! imports and runs as a normal test on the host. This file is only the ABI:
//! allocator, memory plumbing and the two imported functions.

extern crate alloc;

pub mod logic;

use alloc::string::String;
use alloc::vec::Vec;

use logic::Executor;
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

#[unsafe(no_mangle)]
pub extern "C" fn anth_free(ptr: i32, len: i32) {
    if ptr <= 0 || len <= 0 {
        return;
    }
    // SAFETY: the host frees a range it received from `anth_alloc` with the
    // same length, as `docs/design/plugins.md` specifies.
    unsafe {
        drop(Vec::from_raw_parts(
            ptr as *mut u8,
            len as usize,
            len as usize,
        ));
    }
}

/// The host entry point: place a request, get an answer.
#[unsafe(no_mangle)]
pub extern "C" fn anth_handle(ptr: i32, len: i32) -> i64 {
    let request = read_string(ptr, len);
    let mut executor = HostExecutor;
    let answer = logic::handle(&mut executor, &request);
    write_string(answer)
}

fn read_string(ptr: i32, len: i32) -> String {
    if ptr <= 0 || len <= 0 {
        return String::new();
    }
    // SAFETY: the host writes `len` bytes at `ptr` from `anth_alloc` before
    // calling `anth_handle`.
    let bytes = unsafe { core::slice::from_raw_parts(ptr as *const u8, len as usize) };
    String::from_utf8_lossy(bytes).into_owned()
}

fn write_string(text: String) -> i64 {
    let bytes = text.into_bytes();
    let len = bytes.len() as i32;
    let ptr = anth_alloc(len);
    if ptr == 0 {
        return 0;
    }
    // SAFETY: `anth_alloc` just reserved `len` bytes at `ptr`.
    unsafe {
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr as *mut u8, len as usize);
    }
    ((ptr as i64) << 32) | (len as i64)
}

struct HostExecutor;

impl Executor for HostExecutor {
    fn exec(&mut self, capability: &str, args: &Value) -> Result<Value, String> {
        let capability_json = serde_json::to_vec(capability).map_err(|error| error.to_string())?;
        let args_json = serde_json::to_vec(args).map_err(|error| error.to_string())?;

        let packed = unsafe {
            anth_exec(
                capability_json.as_ptr() as i32,
                capability_json.len() as i32,
                args_json.as_ptr() as i32,
                args_json.len() as i32,
            )
        };
        let ptr = (packed >> 32) as i32;
        let len = (packed & 0xffff_ffff) as i32;
        let reply = read_string(ptr, len);

        if reply.is_empty() {
            return Err("the host returned an empty answer".into());
        }
        match serde_json::from_str::<Value>(&reply) {
            Ok(Value::Object(map)) => {
                if let Some(Value::String(detail)) = map.get("error") {
                    return Err(detail.clone());
                }
                Ok(Value::Object(map))
            }
            Ok(other) => Ok(other),
            Err(error) => Err(format!("the host answer is not JSON: {error}")),
        }
    }

    fn log(&mut self, message: &str) {
        unsafe { anth_log(message.as_ptr() as i32, message.len() as i32) }
    }
}
