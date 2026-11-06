//! The execution seam.
//!
//! `anth-runtime` is the only crate that names wasmtime, and the real backend
//! arrives with the first app (§11, M2). What exists here is the contract above
//! that backend: a trait, the values it takes and returns, and a fake
//! implementation the engine's own tests run against.
//!
//! Keeping the seam now is what makes streams a later change inside this crate
//! rather than a platform redesign (§6.3).

use std::collections::BTreeMap;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;

use anth_contract::{id::is_kebab, Error, InterfaceId, World};
use serde_json::Value;

pub mod wasmtime_backend;

pub use wasmtime_backend::WasmtimeRuntime;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// What one instance may spend.
///
/// There is no default on purpose: the values come from the configuration
/// (§6.7) or from the manifest, and a second copy here would drift from both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    pub fuel: u64,
    pub memory_pages: u32,
    pub wall_ms: u64,
}

impl Budget {
    pub const fn new(fuel: u64, memory_pages: u32, wall_ms: u64) -> Self {
        Self {
            fuel,
            memory_pages,
            wall_ms,
        }
    }
}

/// The content and the budget one instance is created from.
#[derive(Clone)]
pub struct InstanceSpec {
    pub app: String,
    pub wasm: Vec<u8>,
    pub budget: Budget,
}

impl fmt::Debug for InstanceSpec {
    /// A component is tens of kilobytes; a log line with the bytes in it is not
    /// a log line, it is a dump.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InstanceSpec")
            .field("app", &self.app)
            .field("wasm_bytes", &self.wasm.len())
            .field("budget", &self.budget)
            .finish()
    }
}

/// A handle the runtime mints. There is no public constructor: an id that a
/// caller invented cannot name an instance, so the type does not allow it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InstanceId(u64);

impl InstanceId {
    pub fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for InstanceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// What instantiation produced: the handle to call, and the world to check.
#[derive(Debug, Clone)]
pub struct Instance {
    pub id: InstanceId,
    pub world: World,
}

/// One call: `interface#function` with an input value.
#[derive(Debug, Clone)]
pub struct Call {
    pub instance: InstanceId,
    pub interface: InterfaceId,
    pub function: String,
    pub input: Value,
}

#[derive(Debug, Clone)]
pub struct Reply {
    pub output: Value,
}

/// The boundary every other crate sees.
///
/// `async fn` in traits is not dyn-compatible, so the futures are boxed by hand
/// rather than pulling a macro crate in for three methods.
pub trait Runtime: Send + Sync {
    /// Instantiates the component and reports the handle plus the world it
    /// declares. The caller cannot guess the id, so it is returned.
    fn instantiate(&self, spec: InstanceSpec) -> BoxFuture<'_, Result<Instance, Error>>;

    /// Calls one exported function. The runtime owns the budget, not the caller.
    fn call(&self, call: Call) -> BoxFuture<'_, Result<Reply, Error>>;

    /// Releases everything the instance holds.
    fn release(&self, instance: InstanceId) -> BoxFuture<'_, ()>;
}

/// A backend that answers from a table. Tests use it so the engine never needs
/// wasmtime, and so the seam is exercised by something other than the real one.
///
/// It models several apps: an answer belongs to an app, an interface and a
/// function, so two apps exporting the same interface do not collide.
#[derive(Debug, Default)]
pub struct FakeRuntime {
    inner: Mutex<FakeState>,
}

#[derive(Debug)]
struct Running {
    app: String,
    budget: Budget,
}

#[derive(Debug, Default)]
struct FakeState {
    /// app → the world its component declares
    worlds: BTreeMap<String, World>,
    /// (app, interface, function) → (output, fuel cost)
    replies: BTreeMap<(String, InterfaceId, String), (Value, u64)>,
    next_instance: u64,
    instances: BTreeMap<InstanceId, Running>,
    calls: usize,
}

impl FakeRuntime {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the world one app reports. A composition has several apps, and each
    /// one declares its own imports and exports.
    pub fn with_world(self, app: &str, world: World) -> Self {
        self.inner
            .lock()
            .expect("fake runtime")
            .worlds
            .insert(app.to_string(), world);
        self
    }

    /// Registers an answer for one app, with the fuel it costs.
    pub fn answer(
        self,
        app: &str,
        interface: InterfaceId,
        function: &str,
        output: Value,
        fuel: u64,
    ) -> Self {
        self.inner.lock().expect("fake runtime").replies.insert(
            (app.to_string(), interface, function.to_string()),
            (output, fuel),
        );
        self
    }

    /// How many calls have been dispatched, for assertions.
    pub fn calls(&self) -> usize {
        self.inner.lock().expect("fake runtime").calls
    }

    pub fn instances(&self) -> usize {
        self.inner.lock().expect("fake runtime").instances.len()
    }

    /// The app an instance belongs to, for assertions about which one answered.
    pub fn app_of(&self, instance: InstanceId) -> Option<String> {
        self.inner
            .lock()
            .expect("fake runtime")
            .instances
            .get(&instance)
            .map(|running| running.app.clone())
    }
}

impl Runtime for FakeRuntime {
    fn instantiate(&self, spec: InstanceSpec) -> BoxFuture<'_, Result<Instance, Error>> {
        Box::pin(async move {
            let mut state = self.inner.lock().expect("fake runtime");
            state.next_instance += 1;
            let id = InstanceId(state.next_instance);
            let world = state.worlds.get(&spec.app).cloned().unwrap_or_default();
            state.instances.insert(
                id,
                Running {
                    app: spec.app,
                    budget: spec.budget,
                },
            );
            Ok(Instance { id, world })
        })
    }

    fn call(&self, call: Call) -> BoxFuture<'_, Result<Reply, Error>> {
        Box::pin(async move {
            if !is_kebab(&call.function) {
                return Err(Error::invalid(format!(
                    "`{}` is not a function name",
                    call.function
                )));
            }
            let mut state = self.inner.lock().expect("fake runtime");
            state.calls += 1;
            let running = state
                .instances
                .get(&call.instance)
                .ok_or_else(|| Error::not_found("no such instance"))?;
            let app = running.app.clone();
            let budget = running.budget;
            let key = (app, call.interface.clone(), call.function.clone());
            let (output, cost) = state.replies.get(&key).cloned().ok_or_else(|| {
                Error::not_found(format!(
                    "`{}#{}` is not exported by this app",
                    call.interface, call.function
                ))
            })?;
            if cost > budget.fuel {
                return Err(Error::exhausted(format!(
                    "`{}#{}` needs {cost} fuel and the budget is {}",
                    call.interface, call.function, budget.fuel
                ))
                .with_detail(serde_json::json!({"budget": "fuel"})));
            }
            Ok(Reply { output })
        })
    }

    fn release(&self, instance: InstanceId) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            self.inner
                .lock()
                .expect("fake runtime")
                .instances
                .remove(&instance);
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::sync::Arc;

    const NOTES: &str = "org.example.notes";
    const OTHER: &str = "org.example.other";

    fn id(text: &str) -> InterfaceId {
        InterfaceId::parse(text).expect("id")
    }

    fn spec(app: &str, fuel: u64) -> InstanceSpec {
        InstanceSpec {
            app: app.to_string(),
            wasm: vec![0, 97, 115, 109],
            budget: Budget::new(fuel, 256, 5_000),
        }
    }

    fn world() -> World {
        World::new(
            [id("anth:store/kv@1.0.0")].into(),
            [id("example:notes/list@1.0.0")].into(),
        )
    }

    fn call(instance: InstanceId, function: &str) -> Call {
        Call {
            instance,
            interface: id("example:notes/list@1.0.0"),
            function: function.to_string(),
            input: serde_json::json!({}),
        }
    }

    #[tokio::test]
    async fn the_runtime_is_usable_behind_a_trait_object() {
        let runtime: Arc<dyn Runtime> = Arc::new(FakeRuntime::new().answer(
            NOTES,
            id("example:notes/list@1.0.0"),
            "all",
            serde_json::json!(1),
            1,
        ));
        let instance = runtime
            .instantiate(spec(NOTES, 1_000))
            .await
            .expect("instantiate");
        let reply = runtime.call(call(instance.id, "all")).await.expect("call");
        assert_eq!(reply.output, 1);
    }

    #[tokio::test]
    async fn an_instance_reports_its_id_and_world() {
        let runtime = FakeRuntime::new().with_world(NOTES, world());
        let instance = runtime
            .instantiate(spec(NOTES, 1_000))
            .await
            .expect("instantiate");
        assert_eq!(instance.id.get(), 1);
        assert_eq!(instance.world, world());
        assert_eq!(runtime.instances(), 1);
        assert_eq!(runtime.app_of(instance.id).as_deref(), Some(NOTES));
    }

    #[tokio::test]
    async fn each_app_reports_its_own_world() {
        let other = World::new(BTreeSet::new(), [id("example:other/run@1.0.0")].into());
        let runtime = FakeRuntime::new()
            .with_world(NOTES, world())
            .with_world(OTHER, other.clone());
        let notes = runtime
            .instantiate(spec(NOTES, 1_000))
            .await
            .expect("notes");
        let other_instance = runtime
            .instantiate(spec(OTHER, 1_000))
            .await
            .expect("other");
        assert_eq!(notes.world, world());
        assert_eq!(other_instance.world, other);
    }

    #[tokio::test]
    async fn the_returned_id_is_the_one_to_call() {
        let runtime = FakeRuntime::new().answer(
            NOTES,
            id("example:notes/list@1.0.0"),
            "all",
            serde_json::json!({"entries": []}),
            10,
        );
        let instance = runtime
            .instantiate(spec(NOTES, 1_000))
            .await
            .expect("instantiate");
        let reply = runtime.call(call(instance.id, "all")).await.expect("call");
        assert_eq!(reply.output, serde_json::json!({"entries": []}));
        assert_eq!(runtime.calls(), 1);
    }

    #[tokio::test]
    async fn two_apps_exporting_the_same_interface_do_not_collide() {
        let runtime = FakeRuntime::new()
            .answer(
                NOTES,
                id("example:notes/list@1.0.0"),
                "all",
                serde_json::json!({"from": NOTES}),
                1,
            )
            .answer(
                OTHER,
                id("example:notes/list@1.0.0"),
                "all",
                serde_json::json!({"from": OTHER}),
                1,
            );
        let notes = runtime
            .instantiate(spec(NOTES, 1_000))
            .await
            .expect("notes");
        let other = runtime
            .instantiate(spec(OTHER, 1_000))
            .await
            .expect("other");

        let first = runtime
            .call(call(notes.id, "all"))
            .await
            .expect("notes call");
        let second = runtime
            .call(call(other.id, "all"))
            .await
            .expect("other call");
        assert_eq!(first.output["from"], NOTES);
        assert_eq!(second.output["from"], OTHER);
    }

    #[tokio::test]
    async fn an_unexported_function_is_not_found() {
        let runtime = FakeRuntime::new();
        let instance = runtime
            .instantiate(spec(NOTES, 1_000))
            .await
            .expect("instantiate");
        let error = runtime
            .call(call(instance.id, "secret"))
            .await
            .expect_err("unknown function");
        assert_eq!(error.kind(), anth_contract::ErrorKind::NotFound);
    }

    #[tokio::test]
    async fn a_malformed_function_name_is_refused() {
        let runtime = FakeRuntime::new();
        let instance = runtime
            .instantiate(spec(NOTES, 1_000))
            .await
            .expect("instantiate");
        let error = runtime
            .call(call(instance.id, "Not A Function"))
            .await
            .expect_err("bad name");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Invalid);
    }

    #[tokio::test]
    async fn a_call_over_budget_is_exhausted() {
        let runtime = FakeRuntime::new().answer(
            NOTES,
            id("example:notes/list@1.0.0"),
            "all",
            serde_json::json!(null),
            5_000,
        );
        let instance = runtime
            .instantiate(spec(NOTES, 1_000))
            .await
            .expect("instantiate");
        let error = runtime
            .call(call(instance.id, "all"))
            .await
            .expect_err("over budget");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Exhausted);
        assert!(error.message().contains("5000"), "{error}");
    }

    #[tokio::test]
    async fn a_released_instance_cannot_be_called() {
        let runtime = FakeRuntime::new().answer(
            NOTES,
            id("example:notes/list@1.0.0"),
            "all",
            serde_json::json!(null),
            1,
        );
        let instance = runtime
            .instantiate(spec(NOTES, 1_000))
            .await
            .expect("instantiate");
        let id = instance.id;
        runtime.release(id).await;
        assert_eq!(runtime.instances(), 0);
        let error = runtime.call(call(id, "all")).await.expect_err("gone");
        assert_eq!(error.kind(), anth_contract::ErrorKind::NotFound);
    }

    #[test]
    fn a_world_round_trips_through_json() {
        let text = serde_json::to_string(&world()).expect("json");
        let back: World = serde_json::from_str(&text).expect("parse");
        assert_eq!(back, world());
    }

    #[test]
    fn the_debug_form_does_not_dump_the_component() {
        let text = format!("{:?}", spec(NOTES, 1_000));
        assert!(text.contains("wasm_bytes: 4"), "{text}");
        assert!(!text.contains("97, 115"), "{text}");
    }
}
