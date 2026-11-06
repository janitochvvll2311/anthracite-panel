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

fn id(text: &str) -> InterfaceId {
    InterfaceId::parse(text).expect("interface")
}

#[tokio::test]
async fn a_component_reports_its_world_and_answers() {
    let runtime = WasmtimeRuntime::new().expect("runtime");
    let instance = runtime
        .instantiate(InstanceSpec {
            app: "org.example.greeting".to_string(),
            wasm: greeting(),
            budget: Budget::new(1_000_000, 16, 5_000),
        })
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
        .instantiate(InstanceSpec {
            app: "org.example.spin".to_string(),
            wasm: spin(),
            budget: Budget::new(1_000, 16, 5_000),
        })
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
