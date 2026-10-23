//! Runtime tests against real wasm.
//!
//! These compile a guest from WAT at test time rather than mocking the engine:
//! a mock would prove the code paths agree with themselves, which is exactly the
//! failure mode `docs/design/v0.2.0.md` lesson 4 records. If the sandbox does not
//! stop a spinning guest here, it does not stop one at all.

use super::*;
use crate::plugins::manifest::Manifest;

/// A guest that answers every pipe with a fixed reply, allocated honestly.
///
/// `anth_exec` is imported but never called, so the happy path also proves the
/// import links.
fn guest(reply: &str) -> String {
    let reply_len = reply.len();
    // WAT strings need their own escaping: a JSON quote is `\22` in a data
    // segment, and a NUL is `\00`.
    let escaped = reply.replace('"', "\\22");
    format!(
        r#"
(module
  (import "env" "anth_exec" (func $exec (param i32 i32 i32 i32) (result i64)))
  (import "env" "anth_log" (func $log (param i32 i32)))
  (memory (export "memory") 1)
  (data (i32.const 1024) "{escaped}")
  (func (export "anth_abi_version") (result i32) (i32.const 2))
  (func (export "anth_alloc") (param $len i32) (result i32)
    ;; A real allocator bumps a pointer; the reply lives at a fixed address, so
    ;; any allocation past it is enough to prove the host reads what it wrote.
    (i32.const 4096))
  (func (export "anth_free") (param i32 i32))
  (func $answer (result i64)
    ;; (ptr << 32) | len, pointing at the fixed reply.
    (i64.or (i64.shl (i64.const 1024) (i64.const 32)) (i64.const {reply_len})))
  (func (export "anth_http") (param i32 i32) (result i64) (call $answer))
  (func (export "anth_cli") (param i32 i32) (result i64) (call $answer))
  (func (export "anth_direct") (param i32 i32) (result i64) (call $answer))
)
"#
    )
}

const MANIFEST: &str = r#"
[plugin]
id = "org.test.guest"
version = "0.2.0"
name = "Guest"
abi = 2

[wasm]
path = "plugin.wasm"
sha256 = "0337900b20e3240510dfedb1152912063ce5f679e162324124832d3325d201b0"

[[capability]]
id = "host.hostname"
program = "hostname"
"#;

fn load(wat: &str) -> Loaded {
    let manifest = Manifest::parse(MANIFEST).expect("the test manifest must parse");
    let wasm = wat::parse_str(wat).expect("the test guest must compile");
    Loaded::load(manifest, &wasm).expect("the test guest must load")
}

/// The envelope the crate actually produces, pinned here so a rename in
/// `anth-plugin` fails this test instead of silently emptying every reply. The
/// host read `value` for a while and every plugin answered `null`.
#[test]
fn the_reply_envelope_uses_body_not_value() {
    let plugin = load(&guest(r#"{"status":200,"body":{"greeting":"hello"}}"#));

    let reply = plugin.call("anth_http", "{}").expect("calling");

    assert_eq!(reply.status, 200);
    assert_eq!(reply.value["greeting"], "hello");
}

/// A `null` reply omits the body entirely, which must read as `null` rather
/// than as a broken envelope.
#[test]
fn a_reply_without_a_body_is_null() {
    let plugin = load(&guest(r#"{"status":204}"#));

    let reply = plugin.call("anth_http", "{}").expect("calling");

    assert_eq!(reply.status, 204);
    assert_eq!(reply.value, serde_json::Value::Null);
}

#[test]
fn a_reply_reaches_the_host() {
    let plugin = load(&guest(r#"{"status":200,"body":{"greeting":"hello"}}"#));

    let reply = plugin.call("anth_http", "{}").expect("calling");

    assert_eq!(reply.status, 200);
    assert_eq!(reply.value["greeting"], "hello");
    assert!(reply.error_kind().is_none());
}

#[test]
fn each_pipe_is_reached_through_its_own_entry() {
    let plugin = load(&guest(r#"{"status":200,"body":null}"#));

    for entry in ["anth_http", "anth_cli", "anth_direct"] {
        let reply = plugin
            .call(entry, "\"hostname\"")
            .unwrap_or_else(|e| panic!("{entry}: {}", e.message));
        assert_eq!(reply.status, 200);
    }
}

/// A guest that answers prose is a broken plugin, and the host must say so
/// rather than pretend it understood.
#[test]
fn an_answer_that_is_not_a_reply_becomes_a_500_that_quotes_it() {
    let plugin = load(&guest("not json at all"));

    let reply = plugin
        .call("anth_http", "{}")
        .expect("the call itself works");

    assert_eq!(reply.status, 500);
    assert!(
        reply.value["error"]
            .as_str()
            .unwrap()
            .contains("not a reply"),
        "got: {:?}",
        reply.value
    );
}

#[test]
fn a_guest_that_spins_is_stopped_by_fuel() {
    let wat = r#"
(module
  (import "env" "anth_exec" (func $exec (param i32 i32 i32 i32) (result i64)))
  (import "env" "anth_log" (func $log (param i32 i32)))
  (memory (export "memory") 1)
  (func (export "anth_abi_version") (result i32) (i32.const 2))
  (func (export "anth_alloc") (param i32) (result i32) (i32.const 4096))
  (func (export "anth_free") (param i32 i32))
  (func (export "anth_http") (param i32 i32) (result i64)
    (loop $forever (br $forever))
    (i64.const 0))
)
"#;
    let plugin = load(wat);

    let error = plugin.call("anth_http", "{}").expect_err("must be stopped");

    assert_eq!(error.kind, anth_error_kind_too_large());
    assert!(
        error.message.contains("instruction budget"),
        "got: {}",
        error.message
    );
}

/// The bound the trap is reported as: the daemon is healthy, the request cost
/// more than one call may.
fn anth_error_kind_too_large() -> anth_wire::ErrorKind {
    anth_wire::ErrorKind::TooLarge
}

#[test]
fn a_guest_cannot_import_a_capability_it_was_not_granted() {
    let wat = r#"
(module
  (import "env" "anth_exec" (func $exec (param i32 i32 i32 i32) (result i64)))
  (import "env" "anth_log" (func $log (param i32 i32)))
  (memory (export "memory") 1)
  (func (export "anth_abi_version") (result i32) (i32.const 2))
  (func (export "anth_alloc") (param i32) (result i32) (i32.const 4096))
  (func (export "anth_free") (param i32 i32))
  (func (export "anth_http") (param i32 i32) (result i64)
    ;; Ask for a capability the manifest never froze. The id lives at 2048.
    (drop (call $exec (i32.const 2048) (i32.const 15) (i32.const 4096) (i32.const 2)))
    (i64.const 0))
  (data (i32.const 2048) "host.ungranted\00")
)
"#;
    let plugin = load(wat);

    let error = plugin.call("anth_http", "{}").expect_err("must be refused");

    assert_eq!(error.kind, anth_wire::ErrorKind::Forbidden);
    assert!(
        error.message.contains("grants no capability"),
        "got: {}",
        error.message
    );
}

#[test]
fn a_guest_importing_something_else_fails_to_instantiate() {
    let wat = r#"
(module
  (import "env" "anth_http_post" (func $nope (param i32 i32) (result i64)))
  (memory (export "memory") 1)
  (func (export "anth_abi_version") (result i32) (i32.const 2))
  (func (export "anth_alloc") (param i32) (result i32) (i32.const 0))
  (func (export "anth_free") (param i32 i32))
  (func (export "anth_http") (param i32 i32) (result i64) (i64.const 0))
)
"#;
    let manifest = Manifest::parse(MANIFEST).expect("parsing");
    let wasm = wat::parse_str(wat).expect("compiling");

    let error = Loaded::load(manifest, &wasm).expect_err("must refuse");

    assert_eq!(error.kind, anth_wire::ErrorKind::Invalid);
    assert!(
        error.message.contains("could not be instantiated"),
        "got: {}",
        error.message
    );
}

#[test]
fn a_guest_without_a_transport_entry_is_refused_at_load() {
    let wat = r#"
(module
  (memory (export "memory") 1)
  (func (export "anth_abi_version") (result i32) (i32.const 2))
  (func (export "anth_alloc") (param i32) (result i32) (i32.const 0))
  (func (export "anth_free") (param i32 i32))
)
"#;
    let manifest = Manifest::parse(MANIFEST).expect("parsing");
    let wasm = wat::parse_str(wat).expect("compiling");

    let error = Loaded::load(manifest, &wasm).expect_err("must refuse");

    assert!(
        error.message.contains("no pipe can reach it"),
        "got: {}",
        error.message
    );
}

/// A guest that speaks another ABI is refused by number at load, not trapped on
/// the first request.
#[test]
fn a_guest_speaking_another_abi_is_refused_at_load() {
    let wat = r#"
(module
  (memory (export "memory") 1)
  (func (export "anth_abi_version") (result i32) (i32.const 1))
  (func (export "anth_alloc") (param i32) (result i32) (i32.const 0))
  (func (export "anth_free") (param i32 i32))
  (func (export "anth_http") (param i32 i32) (result i64) (i64.const 0))
)
"#;
    let manifest = Manifest::parse(MANIFEST).expect("parsing");
    let wasm = wat::parse_str(wat).expect("compiling");

    let error = Loaded::load(manifest, &wasm).expect_err("must refuse");

    assert!(
        error.message.contains("answers ABI 1"),
        "got: {}",
        error.message
    );
}

/// The granted capability runs the real program and its output reaches the
/// guest.
///
/// The guest returns the host's own pointer as its reply, so whatever it prints
/// is what the host reads back. A `500` whose message quotes the hostname is the
/// proof: the value crossed the boundary in both directions.
#[test]
fn a_granted_capability_reaches_the_guest() {
    let wat = r#"
(module
  (import "env" "anth_exec" (func $exec (param i32 i32 i32 i32) (result i64)))
  (import "env" "anth_log" (func $log (param i32 i32)))
  (memory (export "memory") 1)
  (func (export "anth_abi_version") (result i32) (i32.const 2))
  (func (export "anth_alloc") (param i32) (result i32) (i32.const 8192))
  (func (export "anth_free") (param i32 i32))
  (func (export "anth_http") (param i32 i32) (result i64)
    (call $exec (i32.const 2048) (i32.const 13) (i32.const 4096) (i32.const 4)))
  (data (i32.const 2048) "host.hostname")
  (data (i32.const 4096) "null")
)
"#;
    let plugin = load(wat);

    // A container without `hostname` has no grant to exercise; the refusal path
    // is covered by the ungranted test above.
    let real_hostname = std::process::Command::new("hostname").output();
    if real_hostname.is_err() {
        return;
    }

    let reply = plugin
        .call("anth_http", "{}")
        .expect("the call itself works");

    let quoted = reply.value["error"].as_str().unwrap_or_default();
    assert!(
        quoted.contains("not a reply"),
        "the hostname should have come back as an unparseable reply, got: {quoted}"
    );
    assert!(
        quoted.contains(
            &String::from_utf8_lossy(&real_hostname.unwrap().stdout)
                .trim()
                .to_string()
        ),
        "the reply should quote what `hostname` printed, got: {quoted}"
    );
}
