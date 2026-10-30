//! Runtime checks with a hand-written guest, so no fixture binary is needed.

use std::sync::Arc;

use super::*;
use crate::apps::manifest::AppManifest as Manifest;
use crate::log::{Logger, Settings};

/// A guest that answers every HTTP call with a fixed envelope.
///
/// `anth_alloc` is a bump allocator at 4096 and `anth_http` ignores its payload;
/// that is enough to prove the host writes the request, reads the reply and maps
/// the envelope without depending on the Rust guest crate.
const GUEST: &str = r#"
(module
  (memory (export "memory") 1)
  (data (i32.const 256) "{\"status\":200,\"body\":{\"hello\":\"world\"}}")
  (func (export "anth_abi_version") (result i32) i32.const 3)
  (func (export "anth_alloc") (param i32) (result i32)
    i32.const 4096)
  (func (export "anth_http") (param i32 i32) (result i64)
    i64.const 1099511627815)
  (func (export "anth_cli") (param i32 i32) (result i64)
    i64.const 1099511627815)
  (func (export "anth_direct") (param i32 i32) (result i64)
    i64.const 1099511627815)
)
"#;

// (256 << 32) | 46 == 1099511627815; 39 is the JSON length below.
const REPLY_LEN: usize = 39;

fn manifest() -> Manifest {
    Manifest::parse(&format!(
        r#"
manifest_version = 2
[app]
id = "org.anth.fixed"
version = "0.3.0"
name = "Fixed"
[runtime]
kind = "wasm"

[wasm]
path = "wasm/plugin.wasm"
abi = 3
sha256 = "{}"
"#,
        "a".repeat(64)
    ))
    .expect("manifest")
}

fn logger() -> Arc<Logger> {
    Arc::new(Logger::buffer(Settings::default()).0)
}

#[test]
fn an_http_anchor_returns_the_guest_envelope() {
    let wasm = wat::parse_str(GUEST).expect("wat");
    let loaded = Loaded::load(
        manifest(),
        &wasm,
        Arc::new(crate::app_host::exec::DemoExecRunner::new()),
    )
    .expect("load");
    let reply = loaded
        .call(
            "anth_http",
            r#"{"method":"GET","path":"/status"}"#,
            logger(),
            10_000_000,
        )
        .expect("call");
    assert_eq!(reply.status, 200);
    assert_eq!(reply.value["hello"], "world");
    assert_eq!(REPLY_LEN, 39);
}

#[test]
fn a_guest_that_speaks_the_wrong_abi_is_refused() {
    let wasm = wat::parse_str(GUEST.replace("i32.const 3", "i32.const 2")).expect("wat");
    let error = Loaded::load(
        manifest(),
        &wasm,
        Arc::new(crate::app_host::exec::DemoExecRunner::new()),
    )
    .expect_err("abi");
    assert!(error.message.contains("ABI 2"), "{error}");
}

#[test]
fn a_guest_without_an_entry_is_refused() {
    let wasm = wat::parse_str(
        r#"(module
            (memory (export "memory") 1)
            (func (export "anth_abi_version") (result i32) i32.const 3)
            (func (export "anth_alloc") (param i32) (result i32) i32.const 0)
        )"#,
    )
    .expect("wat");
    let error = Loaded::load(
        manifest(),
        &wasm,
        Arc::new(crate::app_host::exec::DemoExecRunner::new()),
    )
    .expect_err("entry");
    assert!(error.message.contains("exports none"), "{error}");
}

// ---------------------------------------------------------------- ABI 4 ---

const V3_GUEST: &str = r#"
(module
  (import "env" "anth_capability_call"
    (func $call (param i32 i32 i32 i32) (result i64)))
  (memory (export "memory") 1)
  (data (i32.const 256) "org.anth.host.info")
  (data (i32.const 300) "{}")
  (data (i32.const 600) "{\"status\":200,\"body\":")
  (func (export "anth_abi_version") (result i32) i32.const 4)
  (func (export "anth_alloc") (param i32) (result i32) i32.const 4096)
  (func (export "anth_http") (param i32 i32) (result i64)
    (local $packed i64)
    (local $ptr i32)
    (local $len i32)
    i32.const 256 i32.const 18 i32.const 300 i32.const 2 call $call
    local.set $packed
    local.get $packed i64.const 32 i64.shr_u i32.wrap_i64 local.set $ptr
    local.get $packed i32.wrap_i64 local.set $len
    i32.const 621 local.get $ptr local.get $len memory.copy
    i32.const 621 local.get $len i32.add
    i32.const 125
    i32.store8
    i64.const 600 i64.const 32 i64.shl
    i32.const 22 local.get $len i32.add i64.extend_i32_u
    i64.or)
)
"#;

fn host_import() -> crate::apps::capability::Import {
    let host = crate::apps::host_catalog::exports().remove(0);
    crate::apps::capability::Import {
        id: host.signature.id,
        version: host.signature.version,
        provider: "host".to_string(),
        input: host.signature.input,
        output: host.signature.output,
    }
}

fn v3_manifest(
    imports: Vec<crate::apps::capability::Import>,
) -> crate::apps::capability::ManifestV3 {
    crate::apps::capability::ManifestV3 {
        manifest_version: 3,
        app: crate::apps::manifest::AppSection {
            id: "org.anth.v3".to_string(),
            alias: None,
            version: "0.4.0".to_string(),
            name: "V3".to_string(),
        },
        runtime: crate::apps::manifest::RuntimeSection {
            kind: crate::apps::manifest::RuntimeKind::Wasm,
        },
        wasm: crate::apps::capability::WasmSectionV3 {
            path: crate::apps::manifest::WASM_FILE.to_string(),
            sha256: "a".repeat(64),
            abi: 4,
        },
        export: Vec::new(),
        import: imports,
        cli: Vec::new(),
        api: Vec::new(),
        ui: Vec::new(),
    }
}

#[test]
fn a_v3_guest_reaches_host_info_through_capability_call() {
    let wasm = wat::parse_str(V3_GUEST).expect("wat");
    let loaded = LoadedV3::load(v3_manifest(vec![host_import()]), &wasm).expect("load");
    let reply = loaded
        .call("anth_http", "{}", logger(), 10_000_000)
        .expect("call");
    assert_eq!(reply.status, 200, "{}", reply.value);
    assert!(reply.value["hostname"].is_string(), "{}", reply.value);
    assert!(reply.value["os"]["kernel"].is_string());
}

#[test]
fn an_unimported_v3_capability_is_refused() {
    let wasm = wat::parse_str(V3_GUEST).expect("wat");
    let loaded = LoadedV3::load(v3_manifest(Vec::new()), &wasm).expect("load");
    let error = loaded
        .call("anth_http", "{}", logger(), 10_000_000)
        .expect_err("refused");
    assert!(error.message.contains("imports no capability"), "{error}");
}

#[test]
fn a_v3_guest_with_the_wrong_abi_is_refused() {
    let wasm = wat::parse_str(V3_GUEST.replace("i32.const 4", "i32.const 3")).expect("wat");
    let error = LoadedV3::load(v3_manifest(vec![host_import()]), &wasm).expect_err("abi");
    assert!(error.message.contains("ABI 3"), "{error}");
}
