//! The wasm ABI, behind a macro so a plugin never writes it by hand.
//!
//! Three exports are the transport entries — `anth_http`, `anth_cli` and
//! `anth_direct` — one per pipe, each taking only its own payload and with no
//! `kind` tag to dispatch on. The host picks the entry from the transport it is
//! serving, so a guest cannot misread one for another and there is no shared
//! `match` to get wrong.
//!
//! Three more exports are the memory contract: `anth_abi_version`, `anth_alloc`
//! and `anth_free`. The host checks the version by value and the others by name
//! and signature before it writes a request.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use serde_json::Value;

use crate::plugin::Plugin;
use crate::request::{Request, Response};
use crate::transport::{CliRequest, HttpRequest};
#[cfg(target_arch = "wasm32")]
use alloc::string::ToString;

/// The ABI this crate implements.
///
/// It must equal `anth_panel::plugins::runtime::ABI_VERSION` and `abi` in
/// `plugin.toml`. The host compares the module's answer and refuses a mismatch
/// by name rather than trapping, which is what makes an upgrade diagnosable.
///
/// Version 2 replaced the single `anth_handle` with one entry per transport, so
/// an `abi = 1` bundle has no `anth_http` for the host to call and is refused
/// rather than silently broken.
pub const ABI_VERSION: i32 = 2;

/// The host imports, as the guest sees them.
///
/// Only wasm has them. On the host target this module is absent and
/// [`HostExecutor`] answers every capability with a refusal, so a plugin's
/// crate can still be tested where no daemon exists.
#[cfg(target_arch = "wasm32")]
mod host {
    #[link(wasm_import_module = "env")]
    unsafe extern "C" {
        /// Runs one frozen capability. JSON in, `(ptr << 32) | len` out.
        pub fn anth_exec(cap_ptr: i32, cap_len: i32, args_ptr: i32, args_len: i32) -> i64;
        /// Appends one line to the daemon log.
        pub fn anth_log(ptr: i32, len: i32);
    }
}

/// The [`crate::exec::Executor`] over the host imports.
///
/// Constructed by the generated entries; a plugin only ever receives a
/// `&mut dyn Executor`, so a test substitutes its own without the plugin's
/// logic knowing.
pub struct HostExecutor;

/// Off wasm there is no host to call.
///
/// Refusing loudly is the honest answer: a test that reaches a capability on the
/// host target has a bug, and a `500` naming it is easier to chase than a panic
/// or a silent `200`.
#[cfg(not(target_arch = "wasm32"))]
impl crate::exec::Executor for HostExecutor {
    fn exec(&mut self, capability: &str, _args: &Value) -> Result<Value, String> {
        Err(alloc::format!(
            "`{capability}` needs the wasm host; inject an executor to test this"
        ))
    }

    fn log(&mut self, message: &str) {
        let _ = message;
    }
}

#[cfg(target_arch = "wasm32")]
impl crate::exec::Executor for HostExecutor {
    fn exec(&mut self, capability: &str, args: &Value) -> Result<Value, String> {
        let capability_json = serde_json::to_vec(capability).map_err(|error| error.to_string())?;
        let args_json = serde_json::to_vec(args).map_err(|error| error.to_string())?;
        let packed = unsafe {
            host::anth_exec(
                capability_json.as_ptr() as i32,
                capability_json.len() as i32,
                args_json.as_ptr() as i32,
                args_json.len() as i32,
            )
        };
        if packed == 0 {
            return Err(String::from(
                "the host refused the capability with no detail",
            ));
        }
        let ptr = (packed >> 32) as i32;
        let len = (packed & 0xFFFF_FFFF) as i32;
        if ptr <= 0 || len <= 0 {
            return Err(String::from("the host answered with an empty payload"));
        }
        let bytes = unsafe { core::slice::from_raw_parts(ptr as *const u8, len as usize) };
        let text = String::from_utf8_lossy(bytes).into_owned();
        free_string(ptr, len);
        serde_json::from_str::<Value>(&text)
            .map_err(|error| alloc::format!("the host answered with invalid JSON: {error}"))
    }

    fn log(&mut self, message: &str) {
        unsafe {
            host::anth_log(message.as_ptr() as i32, message.len() as i32);
        }
    }
}

/// Reads host-written bytes out of guest memory.
pub fn read_string(ptr: i32, len: i32) -> String {
    if ptr <= 0 || len <= 0 {
        return String::new();
    }
    // SAFETY: the caller guarantees `ptr` is a range it received from
    // `anth_alloc` with the same `len`, which the host writes before the call.
    let bytes = unsafe { core::slice::from_raw_parts(ptr as *const u8, len as usize) };
    String::from_utf8_lossy(bytes).into_owned()
}

/// Reserves `len` bytes in guest memory and returns the pointer.
pub fn alloc_string(len: i32) -> i32 {
    if len <= 0 {
        return 0;
    }
    let mut buffer = Vec::<u8>::with_capacity(len as usize);
    let ptr = buffer.as_mut_ptr() as i32;
    core::mem::forget(buffer);
    ptr
}

/// Releases a range previously returned by [`alloc_string`].
pub fn free_string(ptr: i32, len: i32) {
    if ptr <= 0 || len <= 0 {
        return;
    }
    // SAFETY: the range came from `alloc_string` with this exact length.
    unsafe {
        drop(Vec::from_raw_parts(
            ptr as *mut u8,
            len as usize,
            len as usize,
        ));
    }
}

/// Copies `text` into guest memory and returns `(ptr << 32) | len`.
pub fn write_string(text: String) -> i64 {
    let bytes = text.into_bytes();
    let len = bytes.len() as i32;
    let ptr = alloc_string(len);
    if ptr == 0 {
        return 0;
    }
    // SAFETY: `alloc_string` just reserved exactly `len` bytes at `ptr`.
    unsafe {
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr as *mut u8, len as usize);
    }
    ((ptr as i64) << 32) | (len as i64)
}

/// Answers one HTTP anchor.
///
/// A payload that does not decode is a `400`, not a trap: the host reads a trap
/// as a broken plugin and the caller sees nothing.
pub fn answer_http<P: Plugin>(
    plugin: &mut P,
    executor: &mut dyn crate::exec::Executor,
    payload: &str,
) -> String {
    let response = match HttpRequest::parse(payload) {
        Ok(request) => plugin.http(executor, request),
        Err(response) => response,
    };
    response.to_envelope()
}

/// Answers one CLI invocation.
pub fn answer_cli<P: Plugin>(
    plugin: &mut P,
    executor: &mut dyn crate::exec::Executor,
    payload: &str,
) -> String {
    let response = match CliRequest::parse(payload) {
        Ok(request) => plugin.cli(executor, request),
        Err(response) => response,
    };
    response.to_envelope()
}

/// Answers a `Request` the caller already mapped, as `plugin call` can.
pub fn answer_direct<P: Plugin>(
    plugin: &mut P,
    executor: &mut dyn crate::exec::Executor,
    payload: &str,
) -> String {
    let response = match decode_request::<P>(payload) {
        Ok(request) => plugin.direct(executor, request),
        Err(response) => response,
    };
    response.to_envelope()
}

/// Decodes `{"action": "...", "value": ...}` against the plugin's own table.
///
/// An `Action` is a `&'static str`, so a name from a payload can only become one
/// by being found in the plugin's declared actions. Anything else is a `404`
/// naming what the plugin does answer, never a fabricated action.
fn decode_request<P: Plugin>(payload: &str) -> Result<Request, Response> {
    #[derive(serde::Deserialize)]
    struct Wire {
        action: String,
        #[serde(default)]
        value: Value,
    }

    let wire = serde_json::from_str::<Wire>(payload)
        .map_err(|error| Response::bad_request(alloc::format!("not a request: {error}")))?;
    let action = P::actions()
        .iter()
        .find(|action| action.as_str() == wire.action)
        .copied()
        .ok_or_else(|| Response::not_found(alloc::format!("no action `{}`", wire.action)))?;
    Ok(Request::new(action, wire.value))
}

/// Generates the six exports the host requires.
///
/// ```ignore
/// #[derive(Default)]
/// struct Hello;
///
/// impl anth_plugin::Plugin for Hello {
///     fn actions() -> &'static [anth_plugin::Action] {
///         &[GREETING]
///     }
///
///     fn from_http(request: anth_plugin::HttpRequest) -> Result<anth_plugin::Request, anth_plugin::Response> {
///         request.method()?;          // a `400` when the method is unknown
///         Ok(anth_plugin::Request::new(GREETING, serde_json::Value::Null))
///     }
///
///     fn from_cli(request: anth_plugin::CliRequest) -> Result<anth_plugin::Request, anth_plugin::Response> {
///         request.into_request(COMMANDS)
///     }
///
///     fn handle(&mut self, executor: &mut dyn anth_plugin::Executor, request: anth_plugin::Request) -> anth_plugin::Response {
///         match request.action {
///             GREETING => anth_plugin::Response::message("hello"),
///             _ => anth_plugin::Response::not_found(request.action.as_str()),
///         }
///     }
/// }
///
/// anth_plugin::anth_plugin!(Hello);
/// ```
#[macro_export]
macro_rules! anth_plugin {
    ($plugin:ty) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn anth_abi_version() -> i32 {
            $crate::ABI_VERSION
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn anth_alloc(len: i32) -> i32 {
            $crate::alloc_string(len)
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn anth_free(ptr: i32, len: i32) {
            $crate::free_string(ptr, len)
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn anth_http(ptr: i32, len: i32) -> i64 {
            let payload = $crate::read_string(ptr, len);
            let mut plugin = <$plugin>::default();
            $crate::write_string($crate::answer_http(
                &mut plugin,
                &mut $crate::HostExecutor,
                &payload,
            ))
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn anth_cli(ptr: i32, len: i32) -> i64 {
            let payload = $crate::read_string(ptr, len);
            let mut plugin = <$plugin>::default();
            $crate::write_string($crate::answer_cli(
                &mut plugin,
                &mut $crate::HostExecutor,
                &payload,
            ))
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn anth_direct(ptr: i32, len: i32) -> i64 {
            let payload = $crate::read_string(ptr, len);
            let mut plugin = <$plugin>::default();
            $crate::write_string($crate::answer_direct(
                &mut plugin,
                &mut $crate::HostExecutor,
                &payload,
            ))
        }
    };
}
