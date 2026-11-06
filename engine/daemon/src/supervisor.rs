//! The supervisor: which instances exist, and for how long (§6.1).
//!
//! It owns the registry and the lifecycle. The runtime only knows how to
//! instantiate and call; it does not know which app an instance belongs to or
//! when to drop it. That is this module's job, and it is why a caller never sees
//! an `InstanceId` it has to track.
//!
//! Locking, in order:
//!
//! - `barrier`: a read guard for `start` and `stop`, a write guard for
//!   `stop_all`. Starting two different apps does not wait; stopping everything
//!   waits for the starts in flight.
//! - `app_locks`: one mutex per app, because two starts of the *same* app would
//!   otherwise both see the registry empty and leak an instance.
//! - `instances`: guards the map itself, held only for a lookup or an insert and
//!   never across an await, so a call is not blocked by a slow instantiation.

use std::collections::BTreeMap;
use std::sync::Arc;

use anth_contract::{AppId, Error, InterfaceId, World};
use anth_runtime::{InstanceId, Reply, Runtime};
use serde_json::Value;
use tokio::sync::{Mutex, RwLock};

/// What starting an app produced.
#[derive(Debug, Clone)]
pub struct Started {
    pub app: String,
    pub id: InstanceId,
    pub world: World,
}

/// Who is running right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Running {
    pub app: String,
    pub id: InstanceId,
}

pub struct Supervisor {
    runtime: Arc<dyn Runtime>,
    barrier: RwLock<()>,
    app_locks: Mutex<BTreeMap<AppId, Arc<Mutex<()>>>>,
    instances: Mutex<BTreeMap<AppId, InstanceId>>,
}

impl Supervisor {
    pub fn new(runtime: Arc<dyn Runtime>) -> Self {
        Self {
            runtime,
            barrier: RwLock::new(()),
            app_locks: Mutex::new(BTreeMap::new()),
            instances: Mutex::new(BTreeMap::new()),
        }
    }

    /// Instantiates the app. An app runs once: starting it again replaces the
    /// previous instance instead of leaking a second one.
    ///
    /// The entry is removed *before* the runtime is asked, so a failed
    /// instantiation leaves the app stopped rather than pointing at an instance
    /// that no longer exists.
    pub async fn start(&self, spec: anth_runtime::InstanceSpec) -> Result<Started, Error> {
        let app_id = AppId::parse(&spec.app)?;
        let app = spec.app.clone();
        let _barrier = self.barrier.read().await;
        let app_lock = self.app_lock(&app_id).await;
        let _app = app_lock.lock_owned().await;

        let previous = self.instances.lock().await.remove(&app_id);
        if let Some(id) = previous {
            self.runtime.release(id).await;
        }

        let instance = self.runtime.instantiate(spec).await?;

        self.instances.lock().await.insert(app_id, instance.id);
        Ok(Started {
            app,
            id: instance.id,
            world: instance.world,
        })
    }

    /// Calls an exported function on a running app.
    pub async fn call(
        &self,
        app: &str,
        interface: &str,
        function: &str,
        input: Value,
    ) -> Result<Reply, Error> {
        let app_id = AppId::parse(app)?;
        let interface = InterfaceId::parse(interface)?;
        let id = self
            .instances
            .lock()
            .await
            .get(&app_id)
            .copied()
            .ok_or_else(|| Error::not_found(format!("app `{app}` is not running")))?;
        self.runtime
            .call(anth_runtime::Call {
                instance: id,
                interface,
                function: function.to_string(),
                input,
            })
            .await
    }

    /// Stops one app. Returns whether it was running.
    pub async fn stop(&self, app: &str) -> bool {
        let Ok(app_id) = AppId::parse(app) else {
            return false;
        };
        let _barrier = self.barrier.read().await;
        let app_lock = self.app_lock(&app_id).await;
        let _app = app_lock.lock_owned().await;
        let id = self.instances.lock().await.remove(&app_id);
        match id {
            Some(id) => {
                self.runtime.release(id).await;
                true
            }
            None => false,
        }
    }

    /// Stops everything, for an orderly shutdown.
    pub async fn stop_all(&self) {
        let _barrier = self.barrier.write().await;
        let ids: Vec<InstanceId> = {
            let mut instances = self.instances.lock().await;
            let ids = instances.values().copied().collect();
            instances.clear();
            ids
        };
        for id in ids {
            self.runtime.release(id).await;
        }
    }

    /// The mutex that serialises the lifecycle of one app.
    async fn app_lock(&self, app: &AppId) -> Arc<Mutex<()>> {
        self.app_locks
            .lock()
            .await
            .entry(app.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    /// What is running, sorted by app.
    pub async fn running(&self) -> Vec<Running> {
        self.instances
            .lock()
            .await
            .iter()
            .map(|(app, id)| Running {
                app: app.to_string(),
                id: *id,
            })
            .collect()
    }

    pub async fn is_running(&self, app: &str) -> bool {
        let Ok(app_id) = AppId::parse(app) else {
            return false;
        };
        self.instances.lock().await.contains_key(&app_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anth_runtime::{Budget, FakeRuntime, Instance, InstanceSpec};
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;

    const NOTES: &str = "org.example.notes";
    const OTHER: &str = "org.example.other";

    fn budget() -> Budget {
        Budget::new(10_000, 256, 5_000)
    }

    fn test_spec(app: &str, wasm: Vec<u8>, budget: Budget) -> InstanceSpec {
        InstanceSpec::new(app, wasm, budget)
    }

    fn interface() -> InterfaceId {
        InterfaceId::parse("example:notes/list@1.0.0").expect("id")
    }

    fn runtime() -> Arc<FakeRuntime> {
        Arc::new(
            FakeRuntime::new()
                .with_world(NOTES, World::default())
                .answer(
                    NOTES,
                    interface(),
                    "all",
                    serde_json::json!({"entries": []}),
                    10,
                ),
        )
    }

    fn supervisor(runtime: Arc<FakeRuntime>) -> Supervisor {
        Supervisor::new(runtime)
    }

    #[tokio::test]
    async fn starting_an_app_reports_its_handle_and_world() {
        let supervisor = supervisor(runtime());
        let started = supervisor
            .start(test_spec(NOTES, vec![0, 97, 115, 109], budget()))
            .await
            .expect("start");
        assert_eq!(started.app, NOTES);
        assert_eq!(started.world, World::default());
        assert!(supervisor.is_running(NOTES).await);
        assert_eq!(supervisor.running().await.len(), 1);
    }

    #[tokio::test]
    async fn a_running_app_answers_a_call() {
        let supervisor = supervisor(runtime());
        supervisor
            .start(test_spec(NOTES, Vec::new(), budget()))
            .await
            .expect("start");
        let reply = supervisor
            .call(
                NOTES,
                "example:notes/list@1.0.0",
                "all",
                serde_json::json!({}),
            )
            .await
            .expect("call");
        assert_eq!(reply.output, serde_json::json!({"entries": []}));
    }

    #[tokio::test]
    async fn calling_a_stopped_app_is_not_found() {
        let supervisor = supervisor(runtime());
        let error = supervisor
            .call(
                NOTES,
                "example:notes/list@1.0.0",
                "all",
                serde_json::json!({}),
            )
            .await
            .expect_err("not running");
        assert_eq!(error.kind(), anth_contract::ErrorKind::NotFound);
        assert!(error.message().contains("not running"), "{error}");
    }

    #[tokio::test]
    async fn a_malformed_interface_is_refused_before_dispatch() {
        let supervisor = supervisor(runtime());
        supervisor
            .start(test_spec(NOTES, Vec::new(), budget()))
            .await
            .expect("start");
        let error = supervisor
            .call(NOTES, "not an interface", "all", serde_json::json!({}))
            .await
            .expect_err("bad interface");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Invalid);
    }

    #[tokio::test]
    async fn a_malformed_app_id_is_refused() {
        let supervisor = supervisor(runtime());
        let error = supervisor
            .start(test_spec("notes", Vec::new(), budget()))
            .await
            .expect_err("bad app id");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Invalid);
        assert!(supervisor.running().await.is_empty());
    }

    #[tokio::test]
    async fn starting_twice_replaces_the_previous_instance() {
        let runtime = runtime();
        let supervisor = supervisor(Arc::clone(&runtime));
        let first = supervisor
            .start(test_spec(NOTES, Vec::new(), budget()))
            .await
            .expect("first");
        let second = supervisor
            .start(test_spec(NOTES, Vec::new(), budget()))
            .await
            .expect("second");
        assert_ne!(first.id, second.id);
        assert_eq!(supervisor.running().await.len(), 1);
        assert_eq!(runtime.instances(), 1);
    }

    #[tokio::test]
    async fn a_failed_start_leaves_the_app_stopped() {
        let runtime = Arc::new(FailingRuntime);
        let supervisor = Supervisor::new(runtime);
        assert!(supervisor
            .start(test_spec(NOTES, Vec::new(), budget()))
            .await
            .is_err());
        assert!(!supervisor.is_running(NOTES).await);
        let error = supervisor
            .call(
                NOTES,
                "example:notes/list@1.0.0",
                "all",
                serde_json::json!({}),
            )
            .await
            .expect_err("not running");
        assert_eq!(error.kind(), anth_contract::ErrorKind::NotFound);
    }

    #[tokio::test]
    async fn a_failed_restart_leaves_the_app_stopped() {
        let supervisor = Supervisor::new(Arc::new(OnceRuntime::default()));
        supervisor
            .start(test_spec(NOTES, Vec::new(), budget()))
            .await
            .expect("first start");

        let error = supervisor
            .start(test_spec(NOTES, Vec::new(), budget()))
            .await
            .expect_err("the second start fails");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Internal);

        // The old instance was released and the new one never existed: the
        // registry must not point at either.
        assert!(!supervisor.is_running(NOTES).await);
        let error = supervisor
            .call(
                NOTES,
                "example:notes/list@1.0.0",
                "all",
                serde_json::json!({}),
            )
            .await
            .expect_err("nothing is running");
        assert_eq!(error.kind(), anth_contract::ErrorKind::NotFound);
    }

    #[tokio::test]
    async fn concurrent_starts_of_the_same_app_leave_one_instance() {
        let fake = Arc::new(FakeRuntime::new());
        let supervisor = Supervisor::new(Arc::new(SlowRuntime(Arc::clone(&fake))));
        let (a, b) = tokio::join!(
            supervisor.start(test_spec(NOTES, Vec::new(), budget())),
            supervisor.start(test_spec(NOTES, Vec::new(), budget()))
        );
        a.expect("first");
        b.expect("second");
        assert_eq!(supervisor.running().await.len(), 1);
        assert_eq!(
            fake.instances(),
            1,
            "a second instantiation would be an orphan the registry never names"
        );
    }

    #[tokio::test]
    async fn different_apps_start_without_waiting_for_each_other() {
        use std::time::Duration;
        use tokio::sync::Barrier;
        use tokio::time::timeout;

        // The runtime only lets an instantiation finish when *two* of them meet.
        // With a global lifecycle lock the second never starts, the first waits
        // forever, and this test times out; with a per-app lock both meet.
        let fake = Arc::new(FakeRuntime::new());
        let supervisor = Supervisor::new(Arc::new(RendezvousRuntime {
            inner: Arc::clone(&fake),
            barrier: Arc::new(Barrier::new(2)),
        }));

        let both = async {
            tokio::join!(
                supervisor.start(test_spec(NOTES, Vec::new(), budget())),
                supervisor.start(test_spec(OTHER, Vec::new(), budget()))
            )
        };
        let (a, b) = timeout(Duration::from_secs(5), both)
            .await
            .expect("different apps must not serialise: the second never reached the barrier");
        a.expect("notes");
        b.expect("other");
        assert_eq!(supervisor.running().await.len(), 2);
        assert_eq!(fake.instances(), 2);
    }

    #[tokio::test]
    async fn stopping_removes_and_releases() {
        let runtime = runtime();
        let supervisor = supervisor(Arc::clone(&runtime));
        supervisor
            .start(test_spec(NOTES, Vec::new(), budget()))
            .await
            .expect("start");
        assert!(supervisor.stop(NOTES).await);
        assert!(!supervisor.stop(NOTES).await);
        assert!(!supervisor.is_running(NOTES).await);
        assert_eq!(runtime.instances(), 0);
    }

    #[tokio::test]
    async fn stopping_everything_releases_every_instance() {
        let runtime = runtime();
        let supervisor = supervisor(Arc::clone(&runtime));
        supervisor
            .start(test_spec(NOTES, Vec::new(), budget()))
            .await
            .expect("start notes");
        supervisor
            .start(test_spec(OTHER, Vec::new(), budget()))
            .await
            .expect("start other");
        assert_eq!(supervisor.running().await.len(), 2);
        supervisor.stop_all().await;
        assert!(supervisor.running().await.is_empty());
        assert_eq!(runtime.instances(), 0);
    }

    /// A runtime that always refuses to instantiate.
    struct FailingRuntime;

    impl Runtime for FailingRuntime {
        fn instantiate(
            &self,
            _: InstanceSpec,
        ) -> Pin<Box<dyn Future<Output = Result<Instance, Error>> + Send + '_>> {
            Box::pin(async { Err(Error::internal("the component did not load")) })
        }

        fn call(
            &self,
            _: anth_runtime::Call,
        ) -> Pin<Box<dyn Future<Output = Result<Reply, Error>> + Send + '_>> {
            Box::pin(async { Err(Error::internal("nothing is running")) })
        }

        fn release(&self, _: InstanceId) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
            Box::pin(async {})
        }
    }

    /// Blocks an instantiation until two of them are in flight.
    struct RendezvousRuntime {
        inner: Arc<FakeRuntime>,
        barrier: Arc<tokio::sync::Barrier>,
    }

    impl Runtime for RendezvousRuntime {
        fn instantiate(
            &self,
            spec: InstanceSpec,
        ) -> Pin<Box<dyn Future<Output = Result<Instance, Error>> + Send + '_>> {
            Box::pin(async move {
                self.barrier.wait().await;
                self.inner.instantiate(spec).await
            })
        }

        fn call(
            &self,
            call: anth_runtime::Call,
        ) -> Pin<Box<dyn Future<Output = Result<Reply, Error>> + Send + '_>> {
            self.inner.call(call)
        }

        fn release(&self, instance: InstanceId) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
            self.inner.release(instance)
        }
    }

    /// Succeeds once, then fails: the shape of a restart that does not load.
    #[derive(Default)]
    struct OnceRuntime {
        inner: FakeRuntime,
        attempts: std::sync::atomic::AtomicUsize,
    }

    impl Runtime for OnceRuntime {
        fn instantiate(
            &self,
            spec: InstanceSpec,
        ) -> Pin<Box<dyn Future<Output = Result<Instance, Error>> + Send + '_>> {
            let attempt = self
                .attempts
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Box::pin(async move {
                if attempt == 0 {
                    self.inner.instantiate(spec).await
                } else {
                    Err(Error::internal("the component did not load"))
                }
            })
        }

        fn call(
            &self,
            call: anth_runtime::Call,
        ) -> Pin<Box<dyn Future<Output = Result<Reply, Error>> + Send + '_>> {
            self.inner.call(call)
        }

        fn release(&self, instance: InstanceId) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
            self.inner.release(instance)
        }
    }

    /// A runtime that yields before instantiating, so two concurrent starts have
    /// a window to interleave if the supervisor does not serialise them.
    struct SlowRuntime(Arc<FakeRuntime>);

    impl Runtime for SlowRuntime {
        fn instantiate(
            &self,
            spec: InstanceSpec,
        ) -> Pin<Box<dyn Future<Output = Result<Instance, Error>> + Send + '_>> {
            Box::pin(async move {
                tokio::task::yield_now().await;
                self.0.instantiate(spec).await
            })
        }

        fn call(
            &self,
            call: anth_runtime::Call,
        ) -> Pin<Box<dyn Future<Output = Result<Reply, Error>> + Send + '_>> {
            self.0.call(call)
        }

        fn release(&self, instance: InstanceId) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
            self.0.release(instance)
        }
    }
}
