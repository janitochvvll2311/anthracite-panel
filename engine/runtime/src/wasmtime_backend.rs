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
    world: "host-runtime",
});

pub struct WasmtimeRuntime {
    instances: Mutex<BTreeMap<u64, Arc<Mutex<Running>>>>,
    next: Mutex<u64>,
}

#[derive(Debug)]
struct StoreData {
    limits: StoreLimits,
    app: String,
    data: Option<std::path::PathBuf>,
    log: crate::LogSettings,
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
        // The monotonic clock is the process's, not the first call's.
        process_start();
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
            let expected = World::new(spec.granted.clone(), spec.promised.clone());
            if let Err(mismatch) = world.check_expected(&expected) {
                return Err(mismatch.into());
            }
            let limits = StoreLimitsBuilder::new()
                .memory_size(spec.budget.memory_pages as usize * 0x10000)
                .build();
            let mut store = Store::new(
                &engine,
                StoreData {
                    limits,
                    app: spec.app.clone(),
                    data: spec.data.clone(),
                    log: spec.log,
                },
            );
            store.limiter(|data| &mut data.limits);
            store
                .set_fuel(spec.budget.fuel)
                .map_err(|error| Error::internal(format!("setting fuel: {error}")))?;
            store.set_epoch_deadline(1);
            let instance = {
                let mut linker = Linker::<StoreData>::new(&engine);
                for id in &spec.granted {
                    link_host(&mut linker, id)?;
                }
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

fn link_host(linker: &mut Linker<StoreData>, id: &InterfaceId) -> Result<(), Error> {
    use wasmtime::component::HasSelf;
    let name = id.to_string();
    let result = match name.as_str() {
        "anth:env/info@1.0.0" => {
            anth::env::info::add_to_linker::<_, HasSelf<_>>(linker, |data| data)
        }
        "anth:env/time@1.0.0" => {
            anth::env::time::add_to_linker::<_, HasSelf<_>>(linker, |data| data)
        }
        "anth:env/log@1.0.0" => anth::env::log::add_to_linker::<_, HasSelf<_>>(linker, |data| data),
        "anth:env/random@1.0.0" => {
            anth::env::random::add_to_linker::<_, HasSelf<_>>(linker, |data| data)
        }
        "anth:store/kv@1.0.0" => {
            anth::store::kv::add_to_linker::<_, HasSelf<_>>(linker, |data| data)
        }
        other => {
            return Err(Error::unavailable(format!(
                "`{other}` is in the grant but the host does not link it yet"
            )));
        }
    };
    result.map_err(|error| Error::internal(format!("linking {name}: {error}")))
}

impl anth::env::info::Host for StoreData {
    fn get(&mut self) -> anth::env::info::Facts {
        static FACTS: std::sync::OnceLock<anth::env::info::Facts> = std::sync::OnceLock::new();
        let mut facts = FACTS.get_or_init(host_facts).clone();
        // Uptime is the one fact that must move; the rest are host constants.
        facts.uptime_seconds = uptime_seconds();
        facts
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
        process_start().elapsed().as_millis() as u64
    }
}

impl anth::env::log::Host for StoreData {
    fn write(&mut self, level: anth::env::log::Level, message: String) {
        let level = match level {
            anth::env::log::Level::Error => crate::LogLevel::Error,
            anth::env::log::Level::Warn => crate::LogLevel::Warn,
            anth::env::log::Level::Info => crate::LogLevel::Info,
            anth::env::log::Level::Debug => crate::LogLevel::Debug,
        };
        if self.log.level.allows(level) {
            emit_log(&self.app, self.log.format, level, &message);
        }
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

impl anth::store::kv::Host for StoreData {
    fn get(&mut self, key: String) -> Result<Option<String>, anth::error::kinds::Error> {
        check_key(&key)?;
        Ok(self.load_kv()?.get(&key).cloned())
    }

    fn set(&mut self, key: String, value: String) -> Result<(), anth::error::kinds::Error> {
        check_key(&key)?;
        if value.len() > MAX_VALUE {
            return Err(anth::error::kinds::Error::TooLarge(MAX_VALUE as u64));
        }
        let mut map = self.load_kv()?;
        map.insert(key, value);
        self.save_kv(&map)
    }

    fn delete(&mut self, key: String) -> Result<bool, anth::error::kinds::Error> {
        check_key(&key)?;
        let mut map = self.load_kv()?;
        let removed = map.remove(&key).is_some();
        if removed {
            self.save_kv(&map)?;
        }
        Ok(removed)
    }

    fn list_keys(&mut self, prefix: String) -> Result<Vec<String>, anth::error::kinds::Error> {
        Ok(self
            .load_kv()?
            .keys()
            .filter(|key| key.starts_with(&prefix))
            .cloned()
            .collect())
    }
}

const MAX_KEY: usize = 1024;
const MAX_VALUE: usize = 1024 * 1024;

fn check_key(key: &str) -> Result<(), anth::error::kinds::Error> {
    if key.is_empty() {
        return Err(anth::error::kinds::Error::Invalid(
            "a store key cannot be empty".to_string(),
        ));
    }
    if key.len() > MAX_KEY {
        return Err(anth::error::kinds::Error::TooLarge(MAX_KEY as u64));
    }
    Ok(())
}

impl StoreData {
    fn kv_dir(&self) -> Result<&std::path::Path, anth::error::kinds::Error> {
        self.data.as_deref().ok_or_else(|| {
            anth::error::kinds::Error::Unavailable("this app has no store".to_string())
        })
    }

    fn load_kv(
        &self,
    ) -> Result<std::collections::BTreeMap<String, String>, anth::error::kinds::Error> {
        use std::os::unix::fs::PermissionsExt;
        let dir = self.kv_dir()?;
        std::fs::create_dir_all(dir).map_err(|error| {
            anth::error::kinds::Error::Internal(format!("creating the app store: {error}"))
        })?;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).map_err(|error| {
            anth::error::kinds::Error::Internal(format!(
                "setting the app store permissions: {error}"
            ))
        })?;
        let path = dir.join("kv.json");
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(anth::error::kinds::Error::Internal(
                    "the app store is a symlink".to_string(),
                ));
            }
            Ok(metadata) if metadata.len() > 4 * 1024 * 1024 => {
                return Err(anth::error::kinds::Error::TooLarge(4 * 1024 * 1024));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(std::collections::BTreeMap::new());
            }
            Err(error) => {
                return Err(anth::error::kinds::Error::Internal(format!(
                    "inspecting the app store: {error}"
                )));
            }
        }
        let text = std::fs::read_to_string(&path).map_err(|error| {
            anth::error::kinds::Error::Internal(format!("reading the app store: {error}"))
        })?;
        serde_json::from_str(&text).map_err(|error| {
            anth::error::kinds::Error::Internal(format!("the app store is corrupt: {error}"))
        })
    }

    fn save_kv(
        &self,
        map: &std::collections::BTreeMap<String, String>,
    ) -> Result<(), anth::error::kinds::Error> {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let dir = self.kv_dir()?;
        std::fs::create_dir_all(dir).map_err(|error| {
            anth::error::kinds::Error::Internal(format!("creating the app store: {error}"))
        })?;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).map_err(|error| {
            anth::error::kinds::Error::Internal(format!(
                "setting the app store permissions: {error}"
            ))
        })?;
        let path = dir.join("kv.json");
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(anth::error::kinds::Error::Internal(
                    "the app store is a symlink".to_string(),
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(anth::error::kinds::Error::Internal(format!(
                    "inspecting the app store: {error}"
                )));
            }
        }
        let bytes = serde_json::to_vec(map).map_err(|error| {
            anth::error::kinds::Error::Internal(format!("rendering the app store: {error}"))
        })?;
        let temp = dir.join(format!(
            ".kv.json.tmp.{}.{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = std::fs::remove_file(&temp);
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)
            .map_err(|error| {
                anth::error::kinds::Error::Internal(format!("writing the app store: {error}"))
            })?;
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .and_then(|()| std::fs::rename(&temp, &path))
            .map_err(|error| {
                let _ = std::fs::remove_file(&temp);
                anth::error::kinds::Error::Internal(format!("writing the app store: {error}"))
            })
    }
}

fn process_start() -> std::time::Instant {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    *START.get_or_init(std::time::Instant::now)
}

fn emit_log(app: &str, format: crate::LogFormat, level: crate::LogLevel, message: &str) {
    let at = humantime::format_rfc3339_seconds(std::time::SystemTime::now()).to_string();
    let message = format!("{app}: {}", sanitize(message));
    match format {
        crate::LogFormat::Text => eprintln!("{at} {:<5} {message}", level.name()),
        crate::LogFormat::Json => eprintln!(
            "{{\"at\":\"{at}\",\"level\":\"{}\",\"message\":{}}}",
            level.name().to_ascii_lowercase(),
            serde_json::to_string(&message).unwrap_or_else(|_| "\"\"".to_string())
        ),
    }
}

/// A host log line is untrusted input: control bytes would forge log entries and
/// an unbounded message would flood the collector.
fn sanitize(message: &str) -> String {
    const MAX: usize = 4096;
    message
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .take(MAX)
        .collect()
}

fn host_facts() -> anth::env::info::Facts {
    anth::env::info::Facts {
        hostname: hostname(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        uptime_seconds: uptime_seconds(),
        cpu_count: cpu_count(),
        memory_bytes: memory_bytes(),
    }
}

fn cpu_count() -> u32 {
    // SAFETY: `sysconf` has no preconditions.
    let count = unsafe { libc::sysconf(libc::_SC_NPROCESSORS_ONLN) };
    if count > 0 {
        count as u32
    } else {
        1
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> StoreData {
        StoreData {
            limits: StoreLimitsBuilder::new().build(),
            app: "org.example.test".to_string(),
            data: None,
            log: crate::LogSettings::default(),
        }
    }

    #[test]
    fn the_host_reports_non_empty_facts() {
        let mut data = data();
        let facts = anth::env::info::Host::get(&mut data);
        assert!(!facts.hostname.is_empty());
        assert!(!facts.os.is_empty());
        assert!(!facts.arch.is_empty());
        assert!(facts.cpu_count >= 1);
        assert!(facts.memory_bytes > 0);
    }

    #[test]
    fn the_host_random_is_bounded() {
        let mut data = data();
        let bytes = anth::env::random::Host::bytes(&mut data, 8).expect("random");
        assert_eq!(bytes.len(), 8);
        let error = anth::env::random::Host::bytes(&mut data, 5000).expect_err("too large");
        assert!(matches!(error, anth::error::kinds::Error::TooLarge(5000)));
    }

    #[test]
    fn the_app_store_round_trips_and_persists() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir()
            .join("anthracite-panel")
            .join(format!("anth-kv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut data = StoreData {
            limits: StoreLimitsBuilder::new().build(),
            app: "org.example.test".to_string(),
            data: Some(dir.clone()),
            log: crate::LogSettings::default(),
        };
        anth::store::kv::Host::set(&mut data, "notes/1".to_string(), "hello".to_string())
            .expect("set");
        anth::store::kv::Host::set(&mut data, "other".to_string(), "x".to_string()).expect("set");
        assert_eq!(
            anth::store::kv::Host::get(&mut data, "notes/1".to_string()).expect("get"),
            Some("hello".to_string())
        );
        assert_eq!(
            anth::store::kv::Host::list_keys(&mut data, "notes/".to_string()).expect("list"),
            ["notes/1".to_string()]
        );
        assert!(anth::store::kv::Host::delete(&mut data, "notes/1".to_string()).expect("delete"));
        assert!(!anth::store::kv::Host::delete(&mut data, "notes/1".to_string()).expect("delete"));
        // A new instance over the same directory reads the same store.
        let mut again = StoreData {
            limits: StoreLimitsBuilder::new().build(),
            app: "org.example.test".to_string(),
            data: Some(dir.clone()),
            log: crate::LogSettings::default(),
        };
        assert_eq!(
            anth::store::kv::Host::get(&mut again, "other".to_string()).expect("get"),
            Some("x".to_string())
        );
        let mode = std::fs::metadata(dir.join("kv.json"))
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_app_without_a_store_is_unavailable() {
        let mut data = StoreData {
            limits: StoreLimitsBuilder::new().build(),
            app: "org.example.test".to_string(),
            data: None,
            log: crate::LogSettings::default(),
        };
        let error = anth::store::kv::Host::get(&mut data, "x".to_string()).expect_err("no store");
        assert!(matches!(error, anth::error::kinds::Error::Unavailable(_)));
    }

    #[test]
    fn a_log_line_is_sanitized_and_bounded() {
        let dirty = format!("a\nb\u{0007}{}", "x".repeat(5000));
        let clean = sanitize(&dirty);
        assert!(!clean.contains('\n'));
        assert!(!clean.contains('\u{0007}'));
        assert_eq!(clean.chars().count(), 4096);
    }

    #[test]
    fn monotonic_time_never_goes_backwards() {
        let mut data = data();
        let first = anth::env::time::Host::monotonic_ms(&mut data);
        let second = anth::env::time::Host::monotonic_ms(&mut data);
        assert!(second >= first);
    }
}
