//! The wasmtime backend (§6.3, §7.12).
//!
//! The trait above this module knows values, budgets and worlds; only this file
//! names wasmtime. A component is compiled once per instantiation, its real world
//! is read with `component_type()`, and calls are dynamic so an app does not need
//! a codegen step in the engine.
//!
//! The first slice supports the value shapes `hello` needs (scalars, strings,
//! lists and records). Type drift is checked by name and version here; the
//! structural signatures of D-World-Signature are filled once WIT is parsed next
//! to the component.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anth_contract::{Error, ErrorKind, InterfaceId, World};
use wasmtime::component::{Component, Linker, Val};
use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder, Trap};

use crate::{BoxFuture, Budget, Call, Instance, InstanceId, InstanceSpec, Reply, Runtime};

wasmtime::component::bindgen!({
    path: "../wit",
    world: "host-env",
});

pub struct WasmtimeRuntime {
    instances: Mutex<BTreeMap<u64, Arc<Mutex<Running>>>>,
    next: Mutex<u64>,
}

#[derive(Debug)]
struct StoreData {
    limits: StoreLimits,
    app: String,
    started: std::time::Instant,
}

struct Running {
    engine: Engine,
    store: Store<StoreData>,
    instance: wasmtime::component::Instance,
    budget: Budget,
}

impl std::fmt::Debug for Running {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Running")
            .field("budget", &self.budget)
            .finish_non_exhaustive()
    }
}

impl WasmtimeRuntime {
    pub fn new() -> Result<Self, Error> {
        Ok(Self {
            instances: Mutex::new(BTreeMap::new()),
            next: Mutex::new(0),
        })
    }

    /// One engine per instance: wasmtime's epoch is global to an `Engine`, so a
    /// wall deadline that advances it must not reach another app (§6.1).
    fn engine() -> Result<Engine, Error> {
        let mut config = Config::new();
        config.consume_fuel(true);
        config.epoch_interruption(true);
        config.wasm_component_model(true);
        Engine::new(&config)
            .map_err(|error| Error::internal(format!("configuring wasmtime: {error}")))
    }

    fn next_id(&self) -> InstanceId {
        let mut next = self.next.lock().expect("wasmtime ids");
        *next += 1;
        InstanceId(*next)
    }

    /// Reads the component's real imports and exports, keeping only the top-level
    /// names that are interface ids (functions are not interfaces).
    fn world(component: &Component, engine: &Engine) -> World {
        let ty = component.component_type();
        let mut imports = std::collections::BTreeSet::new();
        let mut exports = std::collections::BTreeSet::new();
        for (name, _) in ty.imports(engine) {
            if let Ok(id) = InterfaceId::parse(name) {
                imports.insert(id);
            }
        }
        for (name, _) in ty.exports(engine) {
            if let Ok(id) = InterfaceId::parse(name) {
                exports.insert(id);
            }
        }
        World::new(imports, exports)
    }
}

impl Runtime for WasmtimeRuntime {
    fn instantiate(&self, spec: InstanceSpec) -> BoxFuture<'_, Result<Instance, Error>> {
        Box::pin(async move {
            let engine = Self::engine()?;
            let component = Component::from_binary(&engine, &spec.wasm)
                .map_err(|error| Error::invalid(format!("the component is not valid: {error}")))?;
            let world = Self::world(&component, &engine);
            let limits = StoreLimitsBuilder::new()
                .memory_size(spec.budget.memory_pages as usize * 0x10000)
                .build();
            let mut store = Store::new(
                &engine,
                StoreData {
                    limits,
                    app: spec.app.clone(),
                    started: std::time::Instant::now(),
                },
            );
            store.limiter(|data| &mut data.limits);
            store
                .set_fuel(spec.budget.fuel)
                .map_err(|error| Error::internal(format!("setting fuel: {error}")))?;
            store.set_epoch_deadline(1);
            let instance = {
                let mut linker = Linker::<StoreData>::new(&engine);
                HostEnv::add_to_linker::<_, wasmtime::component::HasSelf<_>>(&mut linker, |data| {
                    data
                })
                .map_err(|error| Error::internal(format!("linking the host: {error}")))?;
                linker
                    .instantiate(&mut store, &component)
                    .map_err(|error| {
                        Error::unavailable(format!("the app did not start: {error}"))
                    })?
            };
            let id = self.next_id();
            self.instances.lock().expect("wasmtime instances").insert(
                id.0,
                Arc::new(Mutex::new(Running {
                    engine,
                    store,
                    instance,
                    budget: spec.budget,
                })),
            );
            Ok(Instance { id, world })
        })
    }

    fn call(&self, call: Call) -> BoxFuture<'_, Result<Reply, Error>> {
        Box::pin(async move {
            let running = self
                .instances
                .lock()
                .expect("wasmtime instances")
                .get(&call.instance.0)
                .cloned()
                .ok_or_else(|| Error::not_found("no such instance"))?;
            // One lock per instance, not one for the whole runtime: two apps do
            // not serialize each other (§6.1).
            let mut running = running.lock().expect("wasmtime instance");
            let running = &mut *running;
            let mut store = &mut running.store;
            store
                .set_fuel(running.budget.fuel)
                .map_err(|error| Error::internal(format!("setting fuel: {error}")))?;
            store.set_epoch_deadline(1);
            let instance = &running.instance;
            let (_, interface_index) = instance
                .get_export(&mut store, None, &call.interface.to_string())
                .ok_or_else(|| {
                    Error::not_found(format!("`{}` is not exported by this app", call.interface))
                })?;
            let func_index = instance
                .get_export_index(&mut store, Some(&interface_index), &call.function)
                .ok_or_else(|| {
                    Error::not_found(format!(
                        "`{}#{}` is not exported by this app",
                        call.interface, call.function
                    ))
                })?;
            let func = instance.get_func(&mut store, func_index).ok_or_else(|| {
                Error::not_found(format!(
                    "`{}#{}` is not a function",
                    call.interface, call.function
                ))
            })?;
            let ty = func.ty(&store);
            let params: Vec<Val> = ty
                .params()
                .map(|(name, ty)| json_to_val(call.input.get(name), ty))
                .collect::<Result<_, _>>()?;
            let mut results = vec![Val::Bool(false); ty.results().len()];
            // The wall backstop advances the engine epoch; the channel cancels
            // the timer when the call finishes first, so it does not leak into
            // the next call.
            let (cancel, cancelled) = std::sync::mpsc::channel();
            let engine = running.engine.clone();
            let wall = running.budget.wall_ms;
            let timer = std::thread::spawn(move || {
                if cancelled.recv_timeout(Duration::from_millis(wall)).is_err() {
                    engine.increment_epoch();
                }
            });
            let result = func.call(&mut store, &params, &mut results);
            let _ = cancel.send(());
            let _ = timer.join();
            result.map_err(trap_error)?;
            let output = match results.as_slice() {
                [] => serde_json::Value::Null,
                [value] => val_to_json(value)?,
                values => serde_json::Value::Array(
                    values
                        .iter()
                        .map(val_to_json)
                        .collect::<Result<Vec<_>, _>>()?,
                ),
            };
            Ok(Reply { output })
        })
    }

    fn release(&self, instance: InstanceId) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            self.instances
                .lock()
                .expect("wasmtime instances")
                .remove(&instance.0);
        })
    }
}

impl anth::env::info::Host for StoreData {
    fn get(&mut self) -> anth::env::info::Facts {
        anth::env::info::Facts {
            hostname: hostname(),
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            uptime_seconds: uptime_seconds(),
            cpu_count: std::thread::available_parallelism()
                .map(|count| count.get() as u32)
                .unwrap_or(1),
            memory_bytes: memory_bytes(),
        }
    }
}

impl anth::env::time::Host for StoreData {
    fn now_ms(&mut self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis() as u64)
            .unwrap_or(0)
    }

    fn monotonic_ms(&mut self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }
}

impl anth::env::log::Host for StoreData {
    fn write(&mut self, level: anth::env::log::Level, message: String) {
        eprintln!("app {} {level:?}: {message}", self.app);
    }
}

impl anth::env::random::Host for StoreData {
    fn bytes(&mut self, count: u32) -> Result<Vec<u8>, anth::error::kinds::Error> {
        const MAX: u32 = 4096;
        if count > MAX {
            return Err(anth::error::kinds::Error::TooLarge(u64::from(count)));
        }
        let mut bytes = vec![0u8; count as usize];
        use std::io::Read;
        std::fs::File::open("/dev/urandom")
            .and_then(|mut file| file.read_exact(&mut bytes))
            .map_err(|error| {
                anth::error::kinds::Error::Internal(format!("reading random bytes: {error}"))
            })?;
        Ok(bytes)
    }
}

impl anth::error::kinds::Host for StoreData {}

fn hostname() -> String {
    let mut buffer = [0u8; 256];
    // SAFETY: the buffer and its length are valid for the call.
    let result =
        unsafe { libc::gethostname(buffer.as_mut_ptr().cast::<libc::c_char>(), buffer.len()) };
    if result != 0 {
        return "unknown".to_string();
    }
    let len = buffer
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(buffer.len());
    String::from_utf8_lossy(&buffer[..len]).into_owned()
}

fn uptime_seconds() -> u64 {
    std::fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|text| text.split_whitespace().next()?.parse::<f64>().ok())
        .map(|seconds| seconds as u64)
        .unwrap_or(0)
}

fn memory_bytes() -> u64 {
    std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|text| {
            text.lines()
                .find_map(|line| line.strip_prefix("MemTotal:"))
                .and_then(|value| value.split_whitespace().next())
                .and_then(|kilobytes| kilobytes.parse::<u64>().ok())
        })
        .map(|kilobytes| kilobytes * 1024)
        .unwrap_or(0)
}

fn trap_error(error: wasmtime::Error) -> Error {
    let trap = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<Trap>().copied());
    match trap {
        Some(Trap::OutOfFuel) => Error::exhausted("the app ran out of fuel")
            .with_detail(serde_json::json!({"budget": "fuel"})),
        Some(Trap::Interrupt) => Error::exhausted("the app exceeded its wall budget")
            .with_detail(serde_json::json!({"budget": "wall"})),
        _ => Error::internal(format!("the call trapped: {error}")),
    }
}

fn json_to_val(
    value: Option<&serde_json::Value>,
    ty: wasmtime::component::Type,
) -> Result<Val, Error> {
    use wasmtime::component::Type;
    let value = value.unwrap_or(&serde_json::Value::Null);
    Ok(match ty {
        Type::Bool => Val::Bool(value.as_bool().ok_or_else(|| bad("bool", value))?),
        Type::U8 => Val::U8(number_unsigned(value, u8::try_from)?),
        Type::U16 => Val::U16(number_unsigned(value, u16::try_from)?),
        Type::U32 => Val::U32(number_unsigned(value, u32::try_from)?),
        Type::U64 => Val::U64(value.as_u64().ok_or_else(|| bad("integer", value))?),
        Type::S8 => Val::S8(number(value, i8::try_from)?),
        Type::S16 => Val::S16(number(value, i16::try_from)?),
        Type::S32 => Val::S32(number(value, i32::try_from)?),
        Type::S64 => Val::S64(value.as_i64().ok_or_else(|| bad("integer", value))?),
        Type::Float32 => Val::Float32(
            value
                .as_f64()
                .map(|value| value as f32)
                .ok_or_else(|| bad("float32", value))?,
        ),
        Type::Float64 => Val::Float64(value.as_f64().ok_or_else(|| bad("float64", value))?),
        Type::Char => {
            let text = value.as_str().ok_or_else(|| bad("char", value))?;
            let mut chars = text.chars();
            let ch = chars.next().ok_or_else(|| bad("char", value))?;
            if chars.next().is_some() {
                return Err(Error::invalid(format!("`{text}` is not one character")));
            }
            Val::Char(ch)
        }
        Type::String => Val::String(
            value
                .as_str()
                .ok_or_else(|| bad("string", value))?
                .to_string(),
        ),
        Type::List(list) => {
            let items = value.as_array().ok_or_else(|| bad("list", value))?;
            Val::List(
                items
                    .iter()
                    .map(|item| json_to_val(Some(item), list.ty()))
                    .collect::<Result<_, _>>()?,
            )
        }
        Type::Record(record) => {
            let object = value.as_object().ok_or_else(|| bad("record", value))?;
            let mut fields = Vec::new();
            for field in record.fields() {
                fields.push((
                    field.name.to_string(),
                    json_to_val(object.get(field.name), field.ty)?,
                ));
            }
            Val::Record(fields)
        }
        Type::Option(option) => {
            if value.is_null() {
                Val::Option(None)
            } else {
                Val::Option(Some(Box::new(json_to_val(Some(value), option.ty())?)))
            }
        }
        Type::Enum(enumeration) => {
            let name = value.as_str().ok_or_else(|| bad("enum", value))?;
            if !enumeration.names().any(|case| case == name) {
                return Err(Error::invalid(format!(
                    "`{name}` is not a case of this enum"
                )));
            }
            Val::Enum(name.to_string())
        }
        other => {
            return Err(Error::invalid(format!(
                "parameter type {other:?} is not supported yet"
            )));
        }
    })
}

fn number<T>(
    value: &serde_json::Value,
    convert: impl Fn(i64) -> Result<T, std::num::TryFromIntError>,
) -> Result<T, Error> {
    let value = value.as_i64().ok_or_else(|| bad("integer", value))?;
    convert(value).map_err(|_| Error::invalid(format!("{value} does not fit the parameter type")))
}

fn number_unsigned<T>(
    value: &serde_json::Value,
    convert: impl Fn(u64) -> Result<T, std::num::TryFromIntError>,
) -> Result<T, Error> {
    let value = value.as_u64().ok_or_else(|| bad("integer", value))?;
    convert(value).map_err(|_| Error::invalid(format!("{value} does not fit the parameter type")))
}

fn bad(wanted: &str, value: &serde_json::Value) -> Error {
    Error::invalid(format!("expected {wanted}, found {value}"))
}

fn val_to_json(value: &Val) -> Result<serde_json::Value, Error> {
    use serde_json::Value as Json;
    Ok(match value {
        Val::Bool(value) => Json::Bool(*value),
        Val::S8(value) => Json::from(*value),
        Val::U8(value) => Json::from(*value),
        Val::S16(value) => Json::from(*value),
        Val::U16(value) => Json::from(*value),
        Val::S32(value) => Json::from(*value),
        Val::U32(value) => Json::from(*value),
        Val::S64(value) => Json::from(*value),
        Val::U64(value) => Json::from(*value),
        Val::Float32(value) => Json::from(*value),
        Val::Float64(value) => Json::from(*value),
        Val::Char(value) => Json::String(value.to_string()),
        Val::String(value) => Json::String(value.clone()),
        Val::List(values) | Val::Tuple(values) => {
            Json::Array(values.iter().map(val_to_json).collect::<Result<_, _>>()?)
        }
        Val::Record(fields) => {
            let mut object = serde_json::Map::new();
            for (name, value) in fields {
                object.insert(name.clone(), val_to_json(value)?);
            }
            Json::Object(object)
        }
        Val::Enum(name) => Json::String(name.clone()),
        Val::Option(value) => match value {
            None => Json::Null,
            Some(value) => val_to_json(value)?,
        },
        Val::Result(Ok(value)) => match value {
            None => Json::Null,
            Some(value) => val_to_json(value)?,
        },
        Val::Result(Err(value)) => {
            return Err(match value {
                Some(value) => app_error(value),
                None => Error::internal("the component returned an error without a value"),
            });
        }
        other => {
            return Err(Error::internal(format!(
                "return type {other:?} is not supported yet"
            )));
        }
    })
}

/// Maps the app-facing `anth:error/kinds` variant to the contract error (§7.11).
fn app_error(value: &Val) -> Error {
    let Val::Variant(case, payload) = value else {
        return Error::internal("the component returned a malformed error");
    };
    let kind = ErrorKind::from_code(case).unwrap_or(ErrorKind::Internal);
    let payload = payload.as_deref();
    let message = match payload {
        Some(Val::String(message)) => message.clone(),
        _ => case.clone(),
    };
    let mut error = Error::new(kind, message);
    if let Some(Val::U64(limit)) = payload {
        error = error.with_detail(serde_json::json!({"limit": limit}));
    }
    error
}
