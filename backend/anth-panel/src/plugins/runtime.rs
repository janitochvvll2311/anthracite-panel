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

/// Ceiling on the total bytes a guest may accumulate through `anth_log`.
///
/// `StoreLimits::memory_size` bounds the guest's *linear* memory. The log is a
/// `Vec<String>` on the host heap, outside that limit, so without a cap a guest
/// can turn a 64 KiB line in a loop into a hundred megabytes the caller then
/// prints. Measured before this cap: 2000 lines of 64 KiB left 131 MB.
const MAX_LOG_BYTES: usize = 64 * 1024;

/// Ceiling on how many capabilities one invocation may run.
///
/// Fuel bounds guest *instructions*, not the host work those instructions ask
/// for. An `anth_exec` costs a few instructions and one subprocess, so a guest
/// looping on a granted capability is an amplifier: measured at 100k spawns in
/// 7 s with fuel never exhausted. This bound is what makes the "a plugin cannot
/// hang the daemon" claim true, since fuel alone does not.
pub const MAX_EXECS_PER_INVOCATION: u32 = 32;

/// Wall-clock ceiling on one invocation, as a second backstop.
///
/// Fuel stops a compute loop and the exec ceiling stops spawn amplification, but
/// neither stops a guest that blocks inside a slow child process. This deadline
/// is checked between host calls and **traps** the guest, so it ends the
/// invocation rather than handing back a refusal the guest can retry. It still
/// cannot interrupt a `run()` already in progress: `CommandRunner` takes no
/// timeout, so a single slow child outlives the deadline and the invocation ends
/// when that call returns.
pub const MAX_INVOCATION: std::time::Duration = std::time::Duration::from_secs(30);

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
    /// The invocation outlived [`MAX_INVOCATION`]. Unlike a refusal returned as
    /// a value, this traps the guest: a guest told "no" would just ask again.
    Deadline,
}

/// Marker the deadline trap carries, matched by `classify_name`.
///
/// A host function can only return a value or trap. Returning a refusal was the
/// bug: the guest looped, the check failed again cheaply, and the invocation ran
/// for 110 s against a 30 s deadline. `wasmi` does not re-export `HostError`, so
/// the trap is a plain `Error` carrying this marker and `classify_name` reads it
/// back, the same way it already recognises a fuel exhaustion.
const DEADLINE_MARKER: &str = "anthracite: deadline exceeded";

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
    /// How many calls the host admitted, including the ones that then failed.
    /// Separate from `execs` because the ceiling has to count attempts: a
    /// failing program costs a spawn too, and it was the cheap way to bypass a
    /// ceiling that only counted successes.
    pub attempts: u32,
    /// Log lines refused once [`MAX_LOG_BYTES`] was reached. Non-zero means
    /// `log` is incomplete, which the caller must say rather than print as if
    /// the guest had said nothing more.
    pub log_dropped: u32,
}

/// Everything a guest can reach. One per invocation, so nothing survives between
/// requests and a guest cannot accumulate authority.
struct Host<'a> {
    plugin: String,
    grant: &'a PluginGrant,
    runner: &'a dyn CommandRunner,
    log: Vec<String>,
    /// Bytes already held by `log`, so the cap does not rescan the vector.
    log_bytes: usize,
    /// Log lines dropped after the cap was reached, reported instead of silently
    /// discarded: a truncated log that looks complete is worse than a loud one.
    log_dropped: u32,
    execs: u32,
    attempts: u32,
    deadline: std::time::Instant,
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
            log_bytes: 0,
            log_dropped: 0,
            execs: 0,
            attempts: 0,
            deadline: std::time::Instant::now() + MAX_INVOCATION,
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
        attempts: store.data().attempts,
        log_dropped: store.data().log_dropped,
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
             -> Result<i64, wasmi::Error> {
                // Every refusal below except the deadline becomes a value the
                // guest can log and answer around. The deadline must trap: a
                // guest told "no" simply asks again, which is exactly the loop
                // the deadline exists to end.
                let reply =
                    match read_string(&mut caller, cap_ptr, cap_len).and_then(|capability| {
                        read_string(&mut caller, args_ptr, args_len)
                            .map(|arguments| (capability, arguments))
                    }) {
                        Ok((capability, arguments)) => {
                            match execute(&mut caller, &capability, &arguments) {
                                Ok(reply) => reply,
                                Err(ExecError::Refused(detail)) => {
                                    serde_json::json!({"ok": false, "error": detail}).to_string()
                                }
                                Err(ExecError::Deadline) => {
                                    return Err(wasmi::Error::new(format!(
                                        "{DEADLINE_MARKER}: {}s",
                                        MAX_INVOCATION.as_secs()
                                    )));
                                }
                            }
                        }
                        Err(detail) => {
                            serde_json::json!({"ok": false, "error": detail}).to_string()
                        }
                    };
                // A refusal is a value, not a trap, so the guest can log it and
                // answer anyway. It still has to travel through guest memory,
                // because that is the only channel the ABI has back.
                Ok(write_string(&mut caller, &reply).unwrap_or(0))
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
                let Ok(line) = read_string(&mut caller, ptr, len) else {
                    return;
                };
                let host = caller.data_mut();
                if host.log_bytes + line.len() > MAX_LOG_BYTES {
                    host.log_dropped += 1;
                    return;
                }
                host.log_bytes += line.len();
                host.log.push(line);
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
/// Why `execute` refused. A refusal is normally a value the guest can log and
/// answer around; a deadline has to trap, or a guest simply asks again.
enum ExecError {
    Refused(String),
    Deadline,
}

impl From<String> for ExecError {
    fn from(value: String) -> Self {
        Self::Refused(value)
    }
}

fn execute(
    caller: &mut Caller<'_, Host<'_>>,
    capability: &str,
    arguments: &str,
) -> Result<String, ExecError> {
    let plugin = caller.data().plugin.clone();
    let Some(recipe) = caller.data().grant.recipe(capability).cloned() else {
        return Err(format!("`{plugin}` is not granted the capability `{capability}`").into());
    };
    if recipe.job.is_some() {
        return Err(format!("`{capability}` is a host job and cannot be called by a guest").into());
    }
    let program = recipe
        .program
        .clone()
        .ok_or_else(|| format!("`{capability}` names no program"))?;

    // Two bounds that fuel cannot provide. Fuel counts guest instructions, and
    // one `anth_exec` costs a handful of them for one subprocess, so without
    // these a loop is an amplifier rather than a cost.
    // Counted when the call is admitted, not when it succeeds. A program that
    // exits non-zero used to return before the increment, so a guest looping on
    // a *failing* capability was bounded by nothing: measured at 502,795 spawns
    // against a promised ceiling of 32. A failing program is also the cheapest
    // one to drive, so the guard was missing where abuse is easiest.
    if caller.data().attempts >= MAX_EXECS_PER_INVOCATION {
        return Err(format!(
            "`{plugin}` exceeded {MAX_EXECS_PER_INVOCATION} capability calls in one invocation"
        )
        .into());
    }
    if std::time::Instant::now() > caller.data().deadline {
        return Err(ExecError::Deadline);
    }
    caller.data_mut().attempts += 1;

    let values: BTreeMap<String, String> = serde_json::from_str(arguments)
        .map_err(|error| format!("arguments must be a JSON object: {error}"))?;
    let argv = render_argv(&recipe.as_capability(), &values)
        .map_err(|error| ExecError::Refused(error.detail))?;
    let borrowed = argv.iter().map(String::as_str).collect::<Vec<_>>();

    let output = caller
        .data()
        .runner
        .run(&program, &borrowed)
        .map_err(|error| match error {
            CommandError::Unavailable { program } => {
                ExecError::Refused(format!("`{program}` is not installed"))
            }
            CommandError::Failed { program, detail } => {
                ExecError::Refused(format!("`{program}` failed: {detail}"))
            }
        })?;
    if !output.ok {
        return Err(format!("`{program}` exited non-zero: {}", output.stderr.trim()).into());
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
    if text.contains(DEADLINE_MARKER) {
        return GuestError::new(
            GuestErrorKind::Deadline,
            format!(
                "`{plugin}` exceeded the {}s budget for one invocation",
                MAX_INVOCATION.as_secs()
            ),
        );
    }
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
