//! The wasm side of a plugin: one sandboxed call into a guest, and the two host
//! functions that guest may use.
//!
//! The model is a kitchen. The daemon is the chef and runs programs; the guest is
//! a helper that may only hand over values. Nothing here executes anything on the
//! guest's behalf except `anth_exec`, and that function renders its argv from the
//! recipe frozen in the grant, never from the manifest.

use std::collections::BTreeMap;

use wasmi::{
    Caller, Config, Engine, Instance, Linker, Module, Store, StoreLimits, StoreLimitsBuilder,
    TypedFunc,
};

use super::manifest::{PluginGrant, render_argv};
use crate::command::{CommandError, CommandRunner};

/// The manifest `abi` this host implements.
pub const ABI_VERSION: i32 = 1;

/// Ceiling on a guest's linear memory. A plugin answering an HTTP request has no
/// business holding more, and this is what stops `memory.grow` from turning a
/// loop into an OOM kill of the daemon.
pub const MAX_GUEST_MEMORY: usize = 64 * 1024 * 1024;

/// Default instruction budget, matching `[plugins] fuel`.
pub const DEFAULT_FUEL: u64 = 50_000_000;

/// Largest string the host will move in either direction. A guest could otherwise
/// return a pointer and a length that makes the host allocate without bound.
const MAX_STRING_BYTES: usize = 4 * 1024 * 1024;

/// A guest refused to do something, could not run, or ran out of fuel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuestError {
    pub kind: GuestErrorKind,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuestErrorKind {
    /// The module is malformed, imports what the host does not provide, or traps.
    Invalid,
    /// The guest asked for a capability it was not granted, or sent arguments
    /// that fail their validators.
    Denied,
    /// The fuel budget ran out. Wasm has no wall-clock interrupt, so fuel is the
    /// only thing that stops a guest that loops.
    OutOfFuel,
}

impl GuestError {
    fn new(kind: GuestErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }
}

impl std::fmt::Display for GuestError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.detail)
    }
}

impl std::error::Error for GuestError {}

/// The reply of one invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuestReply {
    pub body: String,
    pub log: Vec<String>,
    /// How many capabilities the guest actually ran, so the caller can tell a
    /// guest that answered from its own data from one that asked the host
    /// something.
    pub execs: u32,
}

/// Everything a guest can reach. One per invocation, so nothing survives between
/// requests and a guest cannot accumulate authority.
struct Host<'a> {
    plugin: String,
    grant: &'a PluginGrant,
    runner: &'a dyn CommandRunner,
    log: Vec<String>,
    execs: u32,
    limits: StoreLimits,
}

/// Runs one guest invocation.
///
/// `request` is one JSON envelope, as documented in `docs/design/plugins.md`. A
/// `fuel` of zero means [`DEFAULT_FUEL`].
///
/// Blocking. A caller in an async context must wrap this in `spawn_blocking`:
/// fuel bounds the work in *instructions*, but the interpreter still holds the
/// thread for the duration.
pub fn invoke(
    wasm: &[u8],
    plugin: &str,
    grant: &PluginGrant,
    runner: &dyn CommandRunner,
    request: &str,
    fuel: u64,
) -> Result<GuestReply, GuestError> {
    let mut config = Config::default();
    config.consume_fuel(true);
    let engine = Engine::new(&config);
    let module = Module::new(&engine, wasm).map_err(|error| {
        GuestError::new(GuestErrorKind::Invalid, format!("not valid wasm: {error}"))
    })?;

    let limits = StoreLimitsBuilder::new()
        .memory_size(MAX_GUEST_MEMORY)
        .memories(1)
        .tables(1)
        .build();
    let mut store = Store::new(
        &engine,
        Host {
            plugin: plugin.to_string(),
            grant,
            runner,
            log: Vec::new(),
            execs: 0,
            limits,
        },
    );
    store.limiter(|host| &mut host.limits);
    store
        .set_fuel(if fuel == 0 { DEFAULT_FUEL } else { fuel })
        .map_err(|error| {
            GuestError::new(
                GuestErrorKind::Invalid,
                format!("fuel is not metered: {error}"),
            )
        })?;

    let mut linker = Linker::new(&engine);
    define_host_functions(&mut linker)?;
    let instance = linker
        .instantiate_and_start(&mut store, &module)
        .map_err(|error| classify(&store, error))?;

    let declared: i32 = call(&instance, &mut store, "anth_abi_version", ())?;
    if declared != ABI_VERSION {
        return Err(GuestError::new(
            GuestErrorKind::Invalid,
            format!("the module implements ABI {declared}, this host provides {ABI_VERSION}"),
        ));
    }

    let len = i32::try_from(request.len()).map_err(|_| {
        GuestError::new(
            GuestErrorKind::Invalid,
            "the request is too large to hand to a guest",
        )
    })?;
    let alloc: TypedFunc<i32, i32> = export(&instance, &store, "anth_alloc")?;
    let ptr = alloc
        .call(&mut store, len)
        .map_err(|error| classify(&store, error))?;
    let memory = instance
        .get_memory(&store, "memory")
        .ok_or_else(|| GuestError::new(GuestErrorKind::Invalid, "the module exports no memory"))?;
    memory
        .write(&mut store, ptr as usize, request.as_bytes())
        .map_err(|error| {
            GuestError::new(
                GuestErrorKind::Invalid,
                format!("could not write the request: {error}"),
            )
        })?;

    let handle: TypedFunc<(i32, i32), i64> = export(&instance, &store, "anth_handle")?;
    let packed = handle
        .call(&mut store, (ptr, len))
        .map_err(|error| classify(&store, error))?;
    let body = read_packed(&instance, &store, packed)?;

    Ok(GuestReply {
        body,
        log: store.data().log.clone(),
        execs: store.data().execs,
    })
}

/// Confirms a module parses, without running it. Used to report a plugin whose
/// module hashes correctly but cannot be loaded.
pub fn probe(wasm: &[u8]) -> Result<(), GuestError> {
    let engine = Engine::new(&Config::default());
    Module::new(&engine, wasm).map(|_| ()).map_err(|error| {
        GuestError::new(GuestErrorKind::Invalid, format!("not valid wasm: {error}"))
    })
}

fn define_host_functions(linker: &mut Linker<Host<'_>>) -> Result<(), GuestError> {
    linker
        .func_wrap(
            "env",
            "anth_exec",
            |mut caller: Caller<'_, Host<'_>>,
             cap_ptr: i32,
             cap_len: i32,
             args_ptr: i32,
             args_len: i32|
             -> i64 {
                let reply = read_string(&mut caller, cap_ptr, cap_len)
                    .and_then(|capability| {
                        read_string(&mut caller, args_ptr, args_len)
                            .map(|arguments| (capability, arguments))
                    })
                    .and_then(|(capability, arguments)| {
                        execute(&mut caller, &capability, &arguments)
                    })
                    .unwrap_or_else(|error| {
                        serde_json::json!({"ok": false, "error": error}).to_string()
                    });
                // A refusal is a value, not a trap, so the guest can log it and
                // answer anyway. It still has to travel through guest memory,
                // because that is the only channel the ABI has back.
                write_string(&mut caller, &reply).unwrap_or(0)
            },
        )
        .map_err(|error| {
            GuestError::new(
                GuestErrorKind::Invalid,
                format!("could not define anth_exec: {error}"),
            )
        })?;
    linker
        .func_wrap(
            "env",
            "anth_log",
            |mut caller: Caller<'_, Host<'_>>, ptr: i32, len: i32| {
                if let Ok(line) = read_string(&mut caller, ptr, len) {
                    caller.data_mut().log.push(line);
                }
            },
        )
        .map_err(|error| {
            GuestError::new(
                GuestErrorKind::Invalid,
                format!("could not define anth_log: {error}"),
            )
        })?;
    Ok(())
}

/// Renders the frozen recipe and runs it.
///
/// Every refusal here is the boundary working: an id the grant does not carry, a
/// host `job` (which exists precisely so a guest never names a program), an
/// argument the recipe does not declare, a value that fails its validator.
fn execute(
    caller: &mut Caller<'_, Host<'_>>,
    capability: &str,
    arguments: &str,
) -> Result<String, String> {
    let plugin = caller.data().plugin.clone();
    let Some(recipe) = caller.data().grant.recipe(capability).cloned() else {
        return Err(format!(
            "`{plugin}` is not granted the capability `{capability}`"
        ));
    };
    if recipe.job.is_some() {
        return Err(format!(
            "`{capability}` is a host job and cannot be called by a guest"
        ));
    }
    let program = recipe
        .program
        .clone()
        .ok_or_else(|| format!("`{capability}` names no program"))?;

    let values: BTreeMap<String, String> = serde_json::from_str(arguments)
        .map_err(|error| format!("arguments must be a JSON object: {error}"))?;
    let argv = render_argv(&recipe.as_capability(), &values).map_err(|error| error.detail)?;
    let borrowed = argv.iter().map(String::as_str).collect::<Vec<_>>();

    let output = caller
        .data()
        .runner
        .run(&program, &borrowed)
        .map_err(|error| match error {
            CommandError::Unavailable { program } => format!("`{program}` is not installed"),
            CommandError::Failed { program, detail } => format!("`{program}` failed: {detail}"),
        })?;
    if !output.ok {
        return Err(format!(
            "`{program}` exited non-zero: {}",
            output.stderr.trim()
        ));
    }
    caller.data_mut().execs += 1;
    Ok(serde_json::json!({
        "ok": true,
        "stdout": output.stdout,
        "stderr": output.stderr,
    })
    .to_string())
}

fn read_string(caller: &mut Caller<'_, Host<'_>>, ptr: i32, len: i32) -> Result<String, String> {
    if ptr < 0 || len < 0 || len as usize > MAX_STRING_BYTES {
        return Err("an argument range is out of bounds".to_string());
    }
    let memory = caller
        .get_export("memory")
        .and_then(|export| export.into_memory())
        .ok_or_else(|| "the guest exports no memory".to_string())?;
    let mut buffer = vec![0u8; len as usize];
    memory
        .read(&*caller, ptr as usize, &mut buffer)
        .map_err(|error| format!("unreadable argument range: {error}"))?;
    String::from_utf8(buffer).map_err(|_| "an argument was not UTF-8".to_string())
}

fn write_string(caller: &mut Caller<'_, Host<'_>>, value: &str) -> Result<i64, String> {
    let len = i32::try_from(value.len()).map_err(|_| "the reply is too large".to_string())?;
    let alloc = caller
        .get_export("anth_alloc")
        .and_then(|export| export.into_func())
        .ok_or_else(|| "the guest exports no anth_alloc".to_string())?;
    let mut results = [wasmi::Val::I32(0)];
    alloc
        .call(&mut *caller, &[wasmi::Val::I32(len)], &mut results)
        .map_err(|error| format!("anth_alloc failed: {error}"))?;
    let wasmi::Val::I32(ptr) = results[0] else {
        return Err("anth_alloc returned a non-i32".to_string());
    };
    let memory = caller
        .get_export("memory")
        .and_then(|export| export.into_memory())
        .ok_or_else(|| "the guest exports no memory".to_string())?;
    memory
        .write(&mut *caller, ptr as usize, value.as_bytes())
        .map_err(|error| format!("could not write the reply: {error}"))?;
    Ok((i64::from(ptr as u32) << 32) | i64::from(len as u32))
}

/// Reads the `(ptr << 32) | len` pair `anth_handle` returns.
fn read_packed(
    instance: &Instance,
    store: &Store<Host<'_>>,
    packed: i64,
) -> Result<String, GuestError> {
    let ptr = (packed >> 32) as u32 as usize;
    let len = (packed & 0xffff_ffff) as u32 as usize;
    if len > MAX_STRING_BYTES {
        return Err(GuestError::new(
            GuestErrorKind::Invalid,
            format!("the guest returned {len} bytes, more than the {MAX_STRING_BYTES} allowed"),
        ));
    }
    let memory = instance
        .get_memory(store, "memory")
        .ok_or_else(|| GuestError::new(GuestErrorKind::Invalid, "the module exports no memory"))?;
    let mut buffer = vec![0u8; len];
    memory.read(store, ptr, &mut buffer).map_err(|error| {
        GuestError::new(
            GuestErrorKind::Invalid,
            format!("the guest returned an unreadable range: {error}"),
        )
    })?;
    String::from_utf8(buffer).map_err(|_| {
        GuestError::new(
            GuestErrorKind::Invalid,
            "the guest returned bytes that are not UTF-8",
        )
    })
}

fn export<Params, Results>(
    instance: &Instance,
    store: &Store<Host<'_>>,
    name: &str,
) -> Result<TypedFunc<Params, Results>, GuestError>
where
    Params: wasmi::WasmParams,
    Results: wasmi::WasmResults,
{
    instance.get_typed_func(store, name).map_err(|error| {
        GuestError::new(
            GuestErrorKind::Invalid,
            format!("the module does not export `{name}` with the ABI's signature: {error}"),
        )
    })
}

fn call<T, R>(
    instance: &Instance,
    store: &mut Store<Host<'_>>,
    name: &str,
    args: T,
) -> Result<R, GuestError>
where
    T: wasmi::WasmParams,
    R: wasmi::WasmResults,
{
    let function: TypedFunc<T, R> = instance.get_typed_func(&*store, name).map_err(|error| {
        GuestError::new(
            GuestErrorKind::Invalid,
            format!("the module does not export `{name}`: {error}"),
        )
    })?;
    function.call(&mut *store, args).map_err(|error| {
        let plugin = store.data().plugin.clone();
        classify_name(&plugin, error)
    })
}

fn classify(store: &Store<Host<'_>>, error: wasmi::Error) -> GuestError {
    classify_name(&store.data().plugin, error)
}

fn classify_name(plugin: &str, error: wasmi::Error) -> GuestError {
    let text = error.to_string();
    if text.contains("fuel") {
        return GuestError::new(
            GuestErrorKind::OutOfFuel,
            "the plugin ran out of fuel".to_string(),
        );
    }
    GuestError::new(
        GuestErrorKind::Invalid,
        format!("`{plugin}` failed: {text}"),
    )
}

#[cfg(test)]
mod tests;
