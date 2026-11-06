//! The real backend, against components built from the WAT under
//! `tests/fixtures/components` (§6.3, §12).

use anth_contract::{ErrorKind, InterfaceId};
use anth_runtime::{Budget, Call, InstanceSpec, Runtime, WasmtimeRuntime};
use serde_json::json;

fn greeting() -> Vec<u8> {
    include_bytes!("../../../tests/fixtures/components/greeting.wasm").to_vec()
}

fn spin() -> Vec<u8> {
    include_bytes!("../../../tests/fixtures/components/spin.wasm").to_vec()
}

fn forever() -> Vec<u8> {
    include_bytes!("../../../tests/fixtures/components/forever.wasm").to_vec()
}

fn wide() -> Vec<u8> {
    include_bytes!("../../../tests/fixtures/components/wide.wasm").to_vec()
}

fn slow() -> Vec<u8> {
    include_bytes!("../../../tests/fixtures/components/slow.wasm").to_vec()
}

fn clock() -> Vec<u8> {
    include_bytes!("../../../tests/fixtures/components/clock.wasm").to_vec()
}

fn spec(
    app: &str,
    wasm: Vec<u8>,
    budget: Budget,
    granted: &[&str],
    promised: &[&str],
) -> InstanceSpec {
    InstanceSpec::new(app, wasm, budget)
        .granting(
            granted
                .iter()
                .map(|id| InterfaceId::parse(id).expect("granted")),
        )
        .promising(
            promised
                .iter()
                .map(|id| InterfaceId::parse(id).expect("promised")),
        )
}

fn id(text: &str) -> InterfaceId {
    InterfaceId::parse(text).expect("interface")
}

#[tokio::test]
async fn a_component_reports_its_world_and_answers() {
    let runtime = WasmtimeRuntime::new().expect("runtime");
    let instance = runtime
        .instantiate(spec(
            "org.example.greeting",
            greeting(),
            Budget::new(1_000_000, 16, 5_000),
            &[],
            &["example:test/greeting@1.0.0"],
        ))
        .await
        .expect("instantiate");
    assert_eq!(
        instance.world.exports,
        [id("example:test/greeting@1.0.0")].into()
    );
    assert!(instance.world.imports.is_empty());
    let reply = runtime
        .call(Call {
            instance: instance.id,
            interface: id("example:test/greeting@1.0.0"),
            function: "greet".to_string(),
            input: json!({}),
        })
        .await
        .expect("call");
    assert_eq!(reply.output, json!(7));
    runtime.release(instance.id).await;
}

#[tokio::test]
async fn fuel_exhaustion_is_exhausted_not_internal() {
    let runtime = WasmtimeRuntime::new().expect("runtime");
    let instance = runtime
        .instantiate(spec(
            "org.example.spin",
            spin(),
            Budget::new(1_000, 16, 5_000),
            &[],
            &["example:test/spin@1.0.0"],
        ))
        .await
        .expect("instantiate");
    let error = runtime
        .call(Call {
            instance: instance.id,
            interface: id("example:test/spin@1.0.0"),
            function: "spin".to_string(),
            input: json!({}),
        })
        .await
        .expect_err("fuel");
    assert_eq!(error.kind(), ErrorKind::Exhausted, "{error}");
    assert_eq!(
        error.envelope().error.detail.expect("detail")["budget"],
        "fuel"
    );
}

#[tokio::test]
async fn the_fuel_budget_is_per_call_not_per_instance() {
    let runtime = WasmtimeRuntime::new().expect("runtime");
    let instance = runtime
        .instantiate(spec(
            "org.example.spin",
            spin(),
            Budget::new(8_000_000, 16, 5_000),
            &[],
            &["example:test/spin@1.0.0"],
        ))
        .await
        .expect("instantiate");
    for _ in 0..2 {
        let reply = runtime
            .call(Call {
                instance: instance.id,
                interface: id("example:test/spin@1.0.0"),
                function: "spin".to_string(),
                input: json!({}),
            })
            .await
            .expect("each call gets a full budget");
        assert_eq!(reply.output, json!(1_000_000));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_apps_wall_deadline_does_not_kill_another() {
    use std::sync::Arc;
    use std::time::Duration;

    let runtime = Arc::new(WasmtimeRuntime::new().expect("runtime"));
    let forever = runtime
        .instantiate(spec(
            "org.example.forever",
            forever(),
            Budget::new(1_000_000_000, 16, 200),
            &[],
            &["example:test/forever@1.0.0"],
        ))
        .await
        .expect("forever");
    let slow = runtime
        .instantiate(spec(
            "org.example.slow",
            slow(),
            Budget::new(5_000_000_000, 16, 5_000),
            &[],
            &["example:test/slow@1.0.0"],
        ))
        .await
        .expect("slow");
    let blocked = runtime.clone();
    let task = tokio::spawn(async move {
        blocked
            .call(Call {
                instance: forever.id,
                interface: id("example:test/forever@1.0.0"),
                function: "run".to_string(),
                input: json!({}),
            })
            .await
    });
    std::thread::sleep(Duration::from_millis(50));
    let long_running = runtime.clone();
    let slow_task = tokio::spawn(async move {
        long_running
            .call(Call {
                instance: slow.id,
                interface: id("example:test/slow@1.0.0"),
                function: "run".to_string(),
                input: json!({}),
            })
            .await
    });
    let reply = slow_task
        .await
        .expect("task")
        .expect("the slow app still answers");
    assert_eq!(reply.output, json!(300_000_000));
    let error = task.await.expect("task").expect_err("wall");
    assert_eq!(error.kind(), ErrorKind::Exhausted, "{error}");
}

#[tokio::test]
async fn a_u64_above_i64_max_round_trips() {
    let runtime = WasmtimeRuntime::new().expect("runtime");
    let instance = runtime
        .instantiate(spec(
            "org.example.wide",
            wide(),
            Budget::new(100_000, 16, 5_000),
            &[],
            &["example:test/wide@1.0.0"],
        ))
        .await
        .expect("instantiate");
    let reply = runtime
        .call(Call {
            instance: instance.id,
            interface: id("example:test/wide@1.0.0"),
            function: "max".to_string(),
            input: json!({}),
        })
        .await
        .expect("call");
    assert_eq!(reply.output, json!(u64::MAX));
    let reply = runtime
        .call(Call {
            instance: instance.id,
            interface: id("example:test/wide@1.0.0"),
            function: "echo".to_string(),
            input: json!({"value": u64::MAX}),
        })
        .await
        .expect("call with a u64 parameter");
    assert_eq!(reply.output, json!(u64::MAX));
}

#[tokio::test]
async fn an_import_the_grant_does_not_list_is_refused() {
    let runtime = WasmtimeRuntime::new().expect("runtime");
    let error = runtime
        .instantiate(spec(
            "org.example.clock",
            clock(),
            Budget::new(1_000_000, 16, 5_000),
            &[],
            &["example:test/clock@1.0.0"],
        ))
        .await
        .expect_err("ungranted import");
    assert_eq!(error.kind(), ErrorKind::Denied, "{error}");
    assert_eq!(
        error.envelope().error.detail.expect("detail")["extra_imports"][0],
        "anth:env/time@1.0.0"
    );
}

#[tokio::test]
async fn a_missing_promised_export_is_refused() {
    let runtime = WasmtimeRuntime::new().expect("runtime");
    let error = runtime
        .instantiate(spec(
            "org.example.greeting",
            greeting(),
            Budget::new(1_000_000, 16, 5_000),
            &[],
            &["example:test/other@1.0.0"],
        ))
        .await
        .expect_err("missing export");
    assert_eq!(error.kind(), ErrorKind::Denied, "{error}");
    assert_eq!(
        error.envelope().error.detail.expect("detail")["missing_exports"][0],
        "example:test/other@1.0.0"
    );
}

#[tokio::test]
async fn a_component_can_call_the_host_clock() {
    let runtime = WasmtimeRuntime::new().expect("runtime");
    let instance = runtime
        .instantiate(spec(
            "org.example.clock",
            clock(),
            Budget::new(1_000_000, 16, 5_000),
            &["anth:env/time@1.0.0"],
            &["example:test/clock@1.0.0"],
        ))
        .await
        .expect("instantiate");
    assert_eq!(instance.world.imports, [id("anth:env/time@1.0.0")].into());
    let reply = runtime
        .call(Call {
            instance: instance.id,
            interface: id("example:test/clock@1.0.0"),
            function: "run".to_string(),
            input: json!({}),
        })
        .await
        .expect("call");
    let millis = reply.output.as_u64().expect("u64");
    assert!(millis > 1_600_000_000_000, "{millis}");
}

#[tokio::test]
async fn a_wall_overrun_is_exhausted_with_the_wall_budget() {
    let runtime = WasmtimeRuntime::new().expect("runtime");
    let instance = runtime
        .instantiate(spec(
            "org.example.forever",
            forever(),
            Budget::new(1_000_000_000, 16, 50),
            &[],
            &["example:test/forever@1.0.0"],
        ))
        .await
        .expect("instantiate");
    let error = runtime
        .call(Call {
            instance: instance.id,
            interface: id("example:test/forever@1.0.0"),
            function: "run".to_string(),
            input: json!({}),
        })
        .await
        .expect_err("wall");
    assert_eq!(error.kind(), ErrorKind::Exhausted, "{error}");
    assert_eq!(
        error.envelope().error.detail.expect("detail")["budget"],
        "wall"
    );
}
