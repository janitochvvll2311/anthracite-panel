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
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;

use anth_contract::{Error, World};
use serde_json::Value;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// What one instance may spend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    pub fuel: u64,
    pub memory_pages: u32,
    pub wall_ms: u64,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            fuel: 10_000_000,
            memory_pages: 256,
            wall_ms: 5_000,
        }
    }
}

/// The content and the budget one instance is created from.
#[derive(Debug, Clone)]
pub struct InstanceSpec {
    pub app: String,
    pub wasm: Vec<u8>,
    pub budget: Budget,
}

/// An instance the supervisor owns and lends to calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InstanceId(pub u64);

/// One call: `interface#function` with an input value.
#[derive(Debug, Clone)]
pub struct Call {
    pub instance: InstanceId,
    pub interface: String,
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
/// rather than pulling a macro crate in for two methods.
pub trait Runtime: Send + Sync {
    /// Instantiates the component and reports the world it declares.
    fn instantiate(&self, spec: InstanceSpec) -> BoxFuture<'_, Result<World, Error>>;

    /// Calls one exported function. The runtime owns the budget, not the caller.
    fn call(&self, call: Call) -> BoxFuture<'_, Result<Reply, Error>>;

    /// Releases everything the instance holds.
    fn release(&self, instance: InstanceId) -> BoxFuture<'_, ()>;
}

/// A backend that answers from a table. Tests use it so the engine never needs
/// wasmtime, and so the seam is exercised by something other than the real one.
#[derive(Debug, Default)]
pub struct FakeRuntime {
    inner: Mutex<FakeState>,
}

#[derive(Debug, Default)]
struct FakeState {
    world: World,
    /// interface → function → (output, fuel cost)
    replies: BTreeMap<(String, String), (Value, u64)>,
    next_instance: u64,
    instances: BTreeMap<InstanceId, Budget>,
    calls: usize,
}

impl FakeRuntime {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets what the next instantiation reports as its world.
    pub fn with_world(self, world: World) -> Self {
        self.inner.lock().expect("fake runtime").world = world;
        self
    }

    /// Registers an answer, with the fuel it costs.
    pub fn answer(self, interface: &str, function: &str, output: Value, fuel: u64) -> Self {
        self.inner.lock().expect("fake runtime").replies.insert(
            (interface.to_string(), function.to_string()),
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
}

impl Runtime for FakeRuntime {
    fn instantiate(&self, spec: InstanceSpec) -> BoxFuture<'_, Result<World, Error>> {
        Box::pin(async move {
            let mut state = self.inner.lock().expect("fake runtime");
            state.next_instance += 1;
            let id = InstanceId(state.next_instance);
            state.instances.insert(id, spec.budget);
            Ok(state.world.clone())
        })
    }

    fn call(&self, call: Call) -> BoxFuture<'_, Result<Reply, Error>> {
        Box::pin(async move {
            let mut state = self.inner.lock().expect("fake runtime");
            state.calls += 1;
            let budget = *state
                .instances
                .get(&call.instance)
                .ok_or_else(|| Error::not_found("no such instance"))?;
            let key = (call.interface.clone(), call.function.clone());
            let (output, cost) = state.replies.get(&key).cloned().ok_or_else(|| {
                Error::not_found(format!(
                    "`{}` is not exported by this app",
                    format_args!("{}#{}", call.interface, call.function)
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
    use anth_contract::InterfaceId;

    fn spec(fuel: u64) -> InstanceSpec {
        InstanceSpec {
            app: "org.example.notes".to_string(),
            wasm: Vec::new(),
            budget: Budget {
                fuel,
                ..Budget::default()
            },
        }
    }

    fn world() -> World {
        World::new(
            [InterfaceId::parse("anth:store/kv@1.0.0").unwrap()].into(),
            [InterfaceId::parse("example:notes/list@1.0.0").unwrap()].into(),
        )
    }

    #[tokio::test]
    async fn an_instance_reports_its_world() {
        let runtime = FakeRuntime::new().with_world(world());
        let declared = runtime.instantiate(spec(1_000)).await.expect("instantiate");
        assert_eq!(declared, world());
        assert_eq!(runtime.instances(), 1);
    }

    #[tokio::test]
    async fn a_registered_function_answers() {
        let runtime = FakeRuntime::new().answer(
            "example:notes/list@1.0.0",
            "all",
            serde_json::json!({"entries": []}),
            10,
        );
        let _ = runtime.instantiate(spec(1_000)).await.expect("instantiate");
        let reply = runtime
            .call(Call {
                instance: InstanceId(1),
                interface: "example:notes/list@1.0.0".to_string(),
                function: "all".to_string(),
                input: serde_json::json!({}),
            })
            .await
            .expect("call");
        assert_eq!(reply.output, serde_json::json!({"entries": []}));
        assert_eq!(runtime.calls(), 1);
    }

    #[tokio::test]
    async fn an_unexported_function_is_not_found() {
        let runtime = FakeRuntime::new();
        let _ = runtime.instantiate(spec(1_000)).await.expect("instantiate");
        let error = runtime
            .call(Call {
                instance: InstanceId(1),
                interface: "example:notes/list@1.0.0".to_string(),
                function: "secret".to_string(),
                input: serde_json::json!({}),
            })
            .await
            .expect_err("unknown function");
        assert_eq!(error.kind(), anth_contract::ErrorKind::NotFound);
    }

    #[tokio::test]
    async fn a_call_over_budget_is_exhausted() {
        let runtime = FakeRuntime::new().answer(
            "example:notes/list@1.0.0",
            "all",
            serde_json::json!(null),
            5_000,
        );
        let _ = runtime.instantiate(spec(1_000)).await.expect("instantiate");
        let error = runtime
            .call(Call {
                instance: InstanceId(1),
                interface: "example:notes/list@1.0.0".to_string(),
                function: "all".to_string(),
                input: serde_json::json!({}),
            })
            .await
            .expect_err("over budget");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Exhausted);
        assert!(error.message().contains("5000"), "{error}");
    }

    #[tokio::test]
    async fn a_released_instance_cannot_be_called() {
        let runtime = FakeRuntime::new().answer(
            "example:notes/list@1.0.0",
            "all",
            serde_json::json!(null),
            1,
        );
        let _ = runtime.instantiate(spec(1_000)).await.expect("instantiate");
        runtime.release(InstanceId(1)).await;
        assert_eq!(runtime.instances(), 0);
        let error = runtime
            .call(Call {
                instance: InstanceId(1),
                interface: "example:notes/list@1.0.0".to_string(),
                function: "all".to_string(),
                input: serde_json::json!({}),
            })
            .await
            .expect_err("gone");
        assert_eq!(error.kind(), anth_contract::ErrorKind::NotFound);
    }

    #[test]
    fn a_world_round_trips_through_json() {
        let text = serde_json::to_string(&world()).expect("json");
        let back: World = serde_json::from_str(&text).expect("parse");
        assert_eq!(back, world());
    }

    #[test]
    fn the_default_budget_matches_the_configuration() {
        let budget = Budget::default();
        assert_eq!(budget.fuel, 10_000_000);
        assert_eq!(budget.memory_pages, 256);
        assert_eq!(budget.wall_ms, 5_000);
    }
}
