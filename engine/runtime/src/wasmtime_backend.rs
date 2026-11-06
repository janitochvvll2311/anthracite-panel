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
use std::sync::Mutex;

use anth_contract::{Error, InterfaceId, World};
use wasmtime::component::{Component, Linker, Val};
use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder};

use crate::{BoxFuture, Budget, Call, Instance, InstanceId, InstanceSpec, Reply, Runtime};

pub struct WasmtimeRuntime {
    engine: Engine,
    instances: Mutex<BTreeMap<u64, Running>>,
    next: Mutex<u64>,
}

#[derive(Debug)]
struct StoreData {
    limits: StoreLimits,
}

struct Running {
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
        let mut config = Config::new();
        config.consume_fuel(true);
        config.wasm_component_model(true);
        let engine = Engine::new(&config)
            .map_err(|error| Error::internal(format!("configuring wasmtime: {error}")))?;
        Ok(Self {
            engine,
            instances: Mutex::new(BTreeMap::new()),
            next: Mutex::new(0),
        })
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
            let component = Component::from_binary(&self.engine, &spec.wasm)
                .map_err(|error| Error::invalid(format!("the component is not valid: {error}")))?;
            let world = Self::world(&component, &self.engine);
            let linker = Linker::<StoreData>::new(&self.engine);
            let limits = StoreLimitsBuilder::new()
                .memory_size(spec.budget.memory_pages as usize * 0x10000)
                .build();
            let mut store = Store::new(&self.engine, StoreData { limits });
            store.limiter(|data| &mut data.limits);
            store
                .set_fuel(spec.budget.fuel)
                .map_err(|error| Error::internal(format!("setting fuel: {error}")))?;
            let instance = linker
                .instantiate(&mut store, &component)
                .map_err(|error| Error::unavailable(format!("the app did not start: {error}")))?;
            let id = self.next_id();
            self.instances.lock().expect("wasmtime instances").insert(
                id.0,
                Running {
                    store,
                    instance,
                    budget: spec.budget,
                },
            );
            Ok(Instance { id, world })
        })
    }

    fn call(&self, call: Call) -> BoxFuture<'_, Result<Reply, Error>> {
        Box::pin(async move {
            let mut instances = self.instances.lock().expect("wasmtime instances");
            let running = instances
                .get_mut(&call.instance.0)
                .ok_or_else(|| Error::not_found("no such instance"))?;
            let mut store = &mut running.store;
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
            func.call(&mut store, &params, &mut results)
                .map_err(trap_error)?;
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

fn trap_error(error: wasmtime::Error) -> Error {
    let out_of_fuel = error
        .chain()
        .any(|cause| cause.downcast_ref::<wasmtime::Trap>() == Some(&wasmtime::Trap::OutOfFuel));
    if out_of_fuel {
        return Error::exhausted("the app ran out of fuel")
            .with_detail(serde_json::json!({"budget": "fuel"}));
    }
    Error::internal(format!("the call trapped: {error}"))
}

fn json_to_val(
    value: Option<&serde_json::Value>,
    ty: wasmtime::component::Type,
) -> Result<Val, Error> {
    use wasmtime::component::Type;
    let value = value.unwrap_or(&serde_json::Value::Null);
    Ok(match ty {
        Type::Bool => Val::Bool(value.as_bool().ok_or_else(|| bad("bool", value))?),
        Type::U8 => Val::U8(number(value, u8::try_from)?),
        Type::U16 => Val::U16(number(value, u16::try_from)?),
        Type::U32 => Val::U32(number(value, u32::try_from)?),
        Type::U64 => Val::U64(number(value, u64::try_from)?),
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
        Val::Result(Err(_)) => {
            return Err(Error::internal("the component returned an error value"));
        }
        other => {
            return Err(Error::internal(format!(
                "return type {other:?} is not supported yet"
            )));
        }
    })
}
