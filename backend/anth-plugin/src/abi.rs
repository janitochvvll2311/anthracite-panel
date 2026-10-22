//! The wasm ABI, behind a macro so a plugin never writes it by hand.
//!
//! Four exports are the whole contract: `anth_abi_version`, `anth_alloc`,
//! `anth_free` and `anth_handle`. The host checks the first by value and the
//! other three by name and signature before it writes a request, so getting one
//! wrong is a plugin that loads and then refuses every call.
//!
//! [`anth_plugin!`] generates all four from a type implementing [`Guest`], which
//! is the only thing a plugin has to write.

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use serde_json::Value;

use crate::command::Command;
use crate::envelope::Reply;
use crate::exec::Executor;

/// The ABI this crate implements.
///
/// It must equal `anth_panel::plugins::runtime::ABI_VERSION` and `abi` in
/// `plugin.toml`. The host compares the module's answer and refuses a mismatch
/// by name rather than trapping, which is what makes an upgrade diagnosable.
pub const ABI_VERSION: i32 = 1;

/// What a plugin implements: one function, a command in, a reply out.
///
/// Both transports arrive here already folded into [`Command`], so a plugin has
/// one dispatch path. There is no `Request` in this trait on purpose: a plugin
/// that matched on the envelope would need a second table for the CLI, and a
/// route reachable from the browser but not from `plugin call` is a bug the
/// types should not invite.
pub trait Guest {
    fn handle(&mut self, command: Command) -> Reply;
}

/// The host imports, as the guest sees them.
mod host {
    #[link(wasm_import_module = "env")]
    unsafe extern "C" {
        /// Runs one frozen capability. JSON in, `(ptr << 32) | len` out.
        pub fn anth_exec(cap_ptr: i32, cap_len: i32, args_ptr: i32, args_len: i32) -> i64;
        /// Appends one line to the daemon log.
        pub fn anth_log(ptr: i32, len: i32);
    }
}

/// The [`Executor`] over the host imports.
///
/// Constructed by the generated `anth_handle`; a plugin only ever receives the
/// `&mut dyn Executor` trait object, so a test substitutes its own without the
/// plugin's logic knowing.
pub struct HostExecutor;

impl Executor for HostExecutor {
    fn exec(&mut self, capability: &str, args: &Value) -> Result<Value, String> {
        let capability_json = serde_json::to_vec(capability).map_err(|error| error.to_string())?;
        let args_json = serde_json::to_vec(args).map_err(|error| error.to_string())?;

        // SAFETY: both slices are live for the call, and the host copies them
        // before it returns, so the borrows need not outlive it.
        let packed = unsafe {
            host::anth_exec(
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
        // The host reports a refusal as `{"error": "..."}` in an otherwise normal
        // reply, so a refusal is an `Err` here and not a value the plugin has to
        // spot.
        match serde_json::from_str::<Value>(&reply) {
            Ok(Value::Object(map)) => match map.get("error") {
                Some(Value::String(detail)) => Err(detail.clone()),
                _ => Ok(Value::Object(map)),
            },
            Ok(other) => Ok(other),
            Err(error) => Err(alloc::format!("the host answer is not JSON: {error}")),
        }
    }

    fn log(&mut self, message: &str) {
        // SAFETY: the slice is live for the call and the host copies it.
        unsafe { host::anth_log(message.as_ptr() as i32, message.len() as i32) }
    }
}

/// Copies `len` bytes at `ptr` into a `String`.
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

/// Answers one envelope: decode, fold to a command, hand to the plugin, encode.
///
/// A malformed envelope — including a CLI call that names no operation — becomes
/// a `400` rather than a trap, because the host reads a trap as a broken plugin
/// and the caller sees nothing.
pub fn answer<G: Guest>(guest: &mut G, request: &str) -> String {
    let reply = match crate::envelope::decode(request).and_then(|request| request.into_command()) {
        Ok(command) => guest.handle(command),
        Err(reply) => reply,
    };
    reply.to_envelope()
}

/// Generates the four exports the host requires.
///
/// ```ignore
/// #[derive(Default)]
/// struct Hello {
///     router: anth_plugin::Router,
/// }
///
/// impl anth_plugin::Guest for Hello {
///     fn handle(&mut self, command: anth_plugin::Command) -> anth_plugin::Reply {
///         self.router.dispatch(&mut anth_plugin::HostExecutor, &command)
///     }
/// }
///
/// anth_plugin::anth_plugin!(Hello);
/// ```
#[macro_export]
macro_rules! anth_plugin {
    ($guest:ty) => {
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
        pub extern "C" fn anth_handle(ptr: i32, len: i32) -> i64 {
            let request = $crate::read_string(ptr, len);
            let mut guest = <$guest>::default();
            $crate::write_string($crate::answer(&mut guest, &request))
        }
    };
}
