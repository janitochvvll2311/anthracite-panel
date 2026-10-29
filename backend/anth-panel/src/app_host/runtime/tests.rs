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
