//! The sandbox: compile once, run one call at a time.
//!
//! The budget model is deliberately redundant, because each bound stops a
//! different attack and none of them implies the other:
//!
//! - **fuel** bounds instructions, so a guest cannot spin;
//! - **memory** bounds growth, so a guest cannot exhaust the host's RAM;
//! - **a deadline** bounds wall time, so a host import that blocks cannot turn
//!   one call into an unbounded one — fuel alone would keep granting ticks;
//! - **a call budget** bounds how many capabilities one invocation runs, so a
//!   guest cannot turn one request into an unbounded number of spawns.
//!
//! All four are checked by the host. A guest that exceeds any of them gets a
//! `Response`, never a hung daemon.

use anth_wire::{ErrorKind, WireError};
use wasmi::{Config, Engine, Instance, Linker, Module, ResourceLimiter, Store};

pub(crate) mod grant;
mod guest;

pub use guest::Reply;

use guest::Host;

/// Instructions one invocation may burn.
///
/// Generous for real work — the example extension answers in a few thousand — and
/// small enough that a spin loop dies in milliseconds instead of holding a
/// connection open.
pub const FUEL_PER_CALL: u64 = 10_000_000;

/// Linear memory one invocation may grow to: 64 MiB, the same bound the wire
/// uses per frame, so a guest cannot build an answer the host would refuse.
pub const MEMORY_LIMIT: usize = 64 * 1024 * 1024;

/// Wall-clock bound on one invocation.
pub const DEADLINE: std::time::Duration = std::time::Duration::from_secs(30);

/// A loaded, verified extension.
///
/// Holds the compiled module rather than an instance: a fresh instance per call
/// is what makes each invocation's memory and fuel its own.
#[derive(Debug)]
pub struct Loaded {
    pub manifest: anth_extension::manifest::ExtensionManifest,
    module: Module,
    engine: Engine,
}

impl Loaded {
    /// Verifies the module's shape and compiles it.
    ///
    /// Compiling at load means a malformed module is reported once, with the
    /// extension's id, instead of on the first request that reaches it.
    pub fn load(
        manifest: anth_extension::manifest::ExtensionManifest,
        wasm: &[u8],
    ) -> Result<Self, WireError> {
        let engine = engine();
        let module = Module::new(&engine, wasm).map_err(|e| {
            WireError::invalid(format!(
                "extension `{}` is not a valid module: {e}",
                manifest.extension.id
            ))
        })?;

        // Check the exports by name before anything runs: a guest missing one
        // would otherwise fail only on the pipe that happened to need it.
        for required in ["anth_abi_version", "anth_alloc", "memory"] {
            if module.get_export(required).is_none() {
                return Err(WireError::invalid(format!(
                    "extension `{}` does not export `{required}`",
                    manifest.extension.id
                )));
            }
        }
        if module.get_export("anth_http").is_none()
            && module.get_export("anth_cli").is_none()
            && module.get_export("anth_direct").is_none()
        {
            return Err(WireError::invalid(format!(
                "extension `{}` exports none of anth_http, anth_cli or anth_direct, so no pipe can reach it",
                manifest.extension.id
            )));
        }

        let loaded = Self {
            manifest,
            module,
            engine,
        };
        loaded.verify_abi()?;
        Ok(loaded)
    }

    pub fn id(&self) -> &str {
        &self.manifest.extension.id
    }

    /// Runs one entry with one payload.
    ///
    /// `entry` is the transport the host is serving, chosen by the caller and
    /// never by the guest, so a payload cannot be read as the wrong pipe.
    pub fn call(
        &self,
        entry: &str,
        payload: &str,
        log: std::sync::Arc<crate::log::Logger>,
        fuel: u64,
    ) -> Result<Reply, WireError> {
        guest::run(self, entry, payload, log, fuel)
    }

    /// Instantiates once and asks the guest which ABI it speaks.
    ///
    /// This is also the only place the imports are checked: a guest that imports
    /// anything beyond `anth_exec` and `anth_log` fails to link here, which is
    /// what keeps "a extension never gets a host handle" true by construction
    /// rather than by review.
    fn verify_abi(&self) -> Result<(), WireError> {
        // Nothing the guest logs here is meant for an operator: this runs at
        // load, to read a version number.
        let log = std::sync::Arc::new(crate::log::Logger::silent());
        let mut store = self.new_store(log, FUEL_PER_CALL);
        let instance = self.instantiate(&mut store)?;
        let version = instance
            .get_typed_func::<(), i32>(&store, "anth_abi_version")
            .map_err(|e| {
                WireError::invalid(format!("anth_abi_version has the wrong signature: {e}"))
            })?
            .call(&mut store, ())
            .map_err(|e| WireError::invalid(format!("anth_abi_version trapped: {e}")))?;

        let expected = anth_extension::manifest::ABI_VERSION as i32;
        if version != expected {
            return Err(WireError::invalid(format!(
                "extension `{}` answers ABI {version}, this daemon speaks {expected}",
                self.manifest.extension.id
            )));
        }
        Ok(())
    }

    pub(crate) fn new_store(
        &self,
        log: std::sync::Arc<crate::log::Logger>,
        fuel: u64,
    ) -> Store<Host> {
        let mut store = Store::new(&self.engine, Host::new(&self.manifest, log));
        store.limiter(|host| host as &mut dyn ResourceLimiter);
        // A store without fuel would let every instruction run unmeasured, so
        // this is a hard expectation rather than a best effort.
        store
            .set_fuel(fuel.max(1))
            .expect("the engine enables fuel");
        store
    }

    pub(crate) fn instantiate(&self, store: &mut Store<Host>) -> Result<Instance, WireError> {
        let mut linker = Linker::new(&self.engine);

        // Exactly two imports, both host functions. A guest that asks for
        // anything else fails to link, which is what stops it from reaching a
        // host handle by name.
        linker
            .func_wrap(
                "env",
                "anth_exec",
                |mut caller: wasmi::Caller<'_, Host>,
                 cap_ptr: i32,
                 cap_len: i32,
                 args_ptr: i32,
                 args_len: i32|
                 -> i64 {
                    guest::exec(&mut caller, cap_ptr, cap_len, args_ptr, args_len)
                },
            )
            .and_then(|linker| {
                linker.func_wrap(
                    "env",
                    "anth_log",
                    |mut caller: wasmi::Caller<'_, Host>, level: i32, ptr: i32, len: i32| {
                        guest::log(&mut caller, level, ptr, len);
                    },
                )
            })
            .map_err(|e| WireError::internal(format!("building the import table: {e}")))?;

        linker
            .instantiate_and_start(&mut *store, &self.module)
            .map_err(|e| {
                crate::extensions::runtime::instantiation_error(&self.manifest.extension.id, e)
            })
    }
}

fn engine() -> Engine {
    let mut config = Config::default();
    config.consume_fuel(true);
    Engine::new(&config)
}

/// A link failure names the extension and the import, because "unknown import" with
/// no extension is the message an operator cannot act on.
fn instantiation_error(id: &str, error: wasmi::Error) -> WireError {
    WireError::invalid(format!(
        "extension `{id}` could not be instantiated: {error}"
    ))
}

#[cfg(test)]
mod tests;

/// The kind a trap maps to.
///
/// Out of fuel is the caller's problem to see as "too expensive to answer", which
/// is why it is `TooLarge` rather than `Internal`: the daemon is fine, the guest
/// asked for more work than a request may cost.
pub(crate) fn trap_kind(id: &str, error: &wasmi::Error) -> WireError {
    match error.as_trap_code() {
        Some(wasmi::TrapCode::OutOfFuel) => WireError::new(
            ErrorKind::TooLarge,
            "extension_out_of_fuel",
            format!("extension `{id}` used its whole instruction budget"),
        ),
        _ => WireError::internal(format!("extension `{id}` trapped: {error}")),
    }
}
