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

use crate::app_host::exec::ExecRunner;
use crate::apps::capability::{APP_ABI_VERSION, ManifestV3};
use crate::apps::host_catalog::{self, HostExport};
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
    pub manifest: crate::apps::manifest::AppManifest,
    module: Module,
    engine: Engine,
    exec: std::sync::Arc<dyn ExecRunner>,
}

impl Loaded {
    /// Verifies the module's shape and compiles it.
    ///
    /// Compiling at load means a malformed module is reported once, with the
    /// extension's id, instead of on the first request that reaches it.
    pub fn load(
        manifest: crate::apps::manifest::AppManifest,
        wasm: &[u8],
        exec: std::sync::Arc<dyn ExecRunner>,
    ) -> Result<Self, WireError> {
        let engine = engine();
        let module = Module::new(&engine, wasm).map_err(|e| {
            WireError::invalid(format!(
                "extension `{}` is not a valid module: {e}",
                manifest.app.id
            ))
        })?;

        // Check the exports by name before anything runs: a guest missing one
        // would otherwise fail only on the pipe that happened to need it.
        for required in ["anth_abi_version", "anth_alloc", "memory"] {
            if module.get_export(required).is_none() {
                return Err(WireError::invalid(format!(
                    "extension `{}` does not export `{required}`",
                    manifest.app.id
                )));
            }
        }
        if module.get_export("anth_http").is_none()
            && module.get_export("anth_cli").is_none()
            && module.get_export("anth_direct").is_none()
        {
            return Err(WireError::invalid(format!(
                "extension `{}` exports none of anth_http, anth_cli or anth_direct, so no pipe can reach it",
                manifest.app.id
            )));
        }

        let loaded = Self {
            manifest,
            module,
            engine,
            exec,
        };
        loaded.verify_abi()?;
        Ok(loaded)
    }

    pub fn id(&self) -> &str {
        &self.manifest.app.id
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
    /// what keeps "an extension never gets a host handle" true by construction
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

        let expected = crate::apps::manifest::ABI_VERSION as i32;
        if version != expected {
            return Err(WireError::invalid(format!(
                "extension `{}` answers ABI {version}, this daemon speaks {expected}",
                self.manifest.app.id
            )));
        }
        Ok(())
    }

    pub(crate) fn new_store(
        &self,
        log: std::sync::Arc<crate::log::Logger>,
        fuel: u64,
    ) -> Store<Host> {
        let mut store = Store::new(
            &self.engine,
            Host::new(&self.manifest, log, std::sync::Arc::clone(&self.exec)),
        );
        store.limiter(|host| host as &mut dyn ResourceLimiter);
        // A store without fuel would let every instruction run unmeasured, so
        // this is a hard expectation rather than a best effort.
        store
            .set_fuel(fuel.max(1))
            .expect("the engine enables fuel");
        store
    }

    pub(crate) fn instantiate(&self, store: &mut Store<Host>) -> Result<Instance, WireError> {
        let linker = linker(&self.engine)?;
        linker
            .instantiate_and_start(&mut *store, &self.module)
            .map_err(|e| crate::app_host::runtime::instantiation_error(&self.manifest.app.id, e))
    }
}

/// A loaded v3 extension: wasm plus the host imports its manifest resolved.
#[derive(Debug)]
pub struct LoadedV3 {
    pub manifest: ManifestV3,
    module: Module,
    engine: Engine,
    host_exports: Vec<HostExport>,
}

impl LoadedV3 {
    /// Verifies the manifest, resolves imports and compiles the module.
    pub fn load(manifest: ManifestV3, wasm: &[u8]) -> Result<Self, WireError> {
        manifest.validate().map_err(WireError::invalid)?;
        let host_exports = resolve_imports(&manifest)?;
        let engine = engine();
        let module = Module::new(&engine, wasm).map_err(|error| {
            WireError::invalid(format!(
                "extension `{}` is not a valid module: {error}",
                manifest.app.id
            ))
        })?;
        for required in ["anth_abi_version", "anth_alloc", "memory"] {
            if module.get_export(required).is_none() {
                return Err(WireError::invalid(format!(
                    "extension `{}` does not export `{required}`",
                    manifest.app.id
                )));
            }
        }
        if module.get_export("anth_http").is_none()
            && module.get_export("anth_cli").is_none()
            && module.get_export("anth_direct").is_none()
        {
            return Err(WireError::invalid(format!(
                "extension `{}` exports none of anth_http, anth_cli or anth_direct",
                manifest.app.id
            )));
        }
        let loaded = Self {
            manifest,
            module,
            engine,
            host_exports,
        };
        loaded.verify_abi()?;
        Ok(loaded)
    }

    pub fn id(&self) -> &str {
        &self.manifest.app.id
    }

    fn verify_abi(&self) -> Result<(), WireError> {
        let log = std::sync::Arc::new(crate::log::Logger::silent());
        let mut store = self.new_store(log, FUEL_PER_CALL);
        let instance = self.instantiate(&mut store)?;
        let version = instance
            .get_typed_func::<(), i32>(&store, "anth_abi_version")
            .map_err(|error| {
                WireError::invalid(format!("anth_abi_version has the wrong signature: {error}"))
            })?
            .call(&mut store, ())
            .map_err(|error| WireError::invalid(format!("anth_abi_version trapped: {error}")))?;
        if version != APP_ABI_VERSION as i32 {
            return Err(WireError::invalid(format!(
                "extension `{}` answers ABI {version}, this daemon speaks {APP_ABI_VERSION}",
                self.manifest.app.id
            )));
        }
        Ok(())
    }

    pub(crate) fn new_store(
        &self,
        log: std::sync::Arc<crate::log::Logger>,
        fuel: u64,
    ) -> Store<Host> {
        let mut store = Store::new(
            &self.engine,
            Host::new_v3(&self.manifest, self.host_exports.clone(), log),
        );
        store.limiter(|host| host as &mut dyn ResourceLimiter);
        store
            .set_fuel(fuel.max(1))
            .expect("the engine enables fuel");
        store
    }

    pub(crate) fn instantiate(&self, store: &mut Store<Host>) -> Result<Instance, WireError> {
        let linker = linker(&self.engine)?;
        linker
            .instantiate_and_start(&mut *store, &self.module)
            .map_err(|error| instantiation_error(&self.manifest.app.id, error))
    }

    pub fn call(
        &self,
        entry: &str,
        payload: &str,
        log: std::sync::Arc<crate::log::Logger>,
        fuel: u64,
    ) -> Result<Reply, WireError> {
        guest::run_v3(self, entry, payload, log, fuel)
    }
}

fn resolve_imports(manifest: &ManifestV3) -> Result<Vec<HostExport>, WireError> {
    let mut resolved = Vec::new();
    for import in &manifest.import {
        let export = host_catalog::find(&import.id, import.version).ok_or_else(|| {
            WireError::invalid(format!(
                "extension `{}` imports `{}@{}`; the host does not provide it",
                manifest.app.id, import.id, import.version
            ))
        })?;
        let expected = import.signature().hash().map_err(WireError::invalid)?;
        let actual = export.signature.hash().map_err(WireError::invalid)?;
        if expected != actual {
            return Err(WireError::invalid(format!(
                "extension `{}` imports `{}@{}` with a signature the host does not provide",
                manifest.app.id, import.id, import.version
            )));
        }
        resolved.push(export);
    }
    Ok(resolved)
}

fn linker(engine: &Engine) -> Result<Linker<Host>, WireError> {
    let mut linker = Linker::new(engine);
    linker
        .func_wrap(
            "env",
            "anth_exec",
            |mut caller: wasmi::Caller<'_, Host>,
             cap_ptr: i32,
             cap_len: i32,
             args_ptr: i32,
             args_len: i32|
             -> i64 { guest::exec(&mut caller, cap_ptr, cap_len, args_ptr, args_len) },
        )
        .map_err(|e| WireError::internal(format!("building the import table: {e}")))?;
    linker
        .func_wrap(
            "env",
            "anth_capability_call",
            |mut caller: wasmi::Caller<'_, Host>,
             name_ptr: i32,
             name_len: i32,
             args_ptr: i32,
             args_len: i32|
             -> i64 {
                guest::capability(&mut caller, name_ptr, name_len, args_ptr, args_len)
            },
        )
        .map_err(|e| WireError::internal(format!("building the import table: {e}")))?;
    linker
        .func_wrap(
            "env",
            "anth_log",
            |mut caller: wasmi::Caller<'_, Host>, level: i32, ptr: i32, len: i32| {
                guest::log(&mut caller, level, ptr, len);
            },
        )
        .map_err(|e| WireError::internal(format!("building the import table: {e}")))?;
    Ok(linker)
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
