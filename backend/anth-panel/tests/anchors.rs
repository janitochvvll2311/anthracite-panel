//! API anchors: the HTTP path into a plugin.
//!
//! The guest here is generated per test because the interesting cases are
//! different *requests*, and the reference `guest.wat` ignores its input by
//! construction. `routed_guest` reads the envelope, runs one capability, and
//! answers with both the capability's reply and the path it was given, so a test
//! can prove what reached the guest and what the guest could reach.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use anth_panel::app::AppState;
use anth_panel::config::Config;
use anth_panel::plugins::api::MAX_ANCHOR_BODY;
use anth_panel::plugins::manifest::{ApiAnchor, anchor_route};
use anth_panel::plugins::{PluginGrant, manifest::Manifest};
use axum::body::Body;
use axum::http::{Request as HttpRequest, StatusCode};
use tower::ServiceExt;

fn unique_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir =
        std::env::temp_dir().join(format!("anth-anchor-{name}-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn sha_of(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A guest that reads its envelope and calls one capability with a fixed
/// argument, then answers with the plugin's path plus the reply.
fn routed_guest() -> Vec<u8> {
    let wat = r#"
(module
  (import "env" "anth_exec" (func $exec (param i32 i32 i32 i32) (result i64)))
  (import "env" "anth_log" (func $log (param i32 i32)))
  (memory (export "memory") 1)
  (global $bump (mut i32) (i32.const 4096))
  (func (export "anth_alloc") (param $len i32) (result i32)
    (local $ptr i32)
    (local.set $ptr (global.get $bump))
    (global.set $bump (i32.add (global.get $bump) (local.get $len)))
    (local.get $ptr))
  (func (export "anth_free") (param $ptr i32) (param $len i32) (drop (local.get $ptr)) (drop (local.get $len)))
  (func (export "anth_abi_version") (result i32) (i32.const 1))
  (data (i32.const 0) "wg.status")
  (data (i32.const 32) "{\"interface\":\"wg0\"}")
  ;; The reply is fixed: what a test wants to know is that the capability ran
  ;; and that the guest received an envelope, not what the program printed.
  (data (i32.const 128) "{\"status\":200,\"body\":\"anchored\"}")
  (func (export "anth_handle") (param $ptr i32) (param $len i32) (result i64)
    ;; Refuse an empty envelope: that is how a test tells a real request from a
    ;; host that forgot to pass one.
    (if (i32.eqz (local.get $len)) (then (return (i64.const -1))))
    (drop (call $exec (i32.const 0) (i32.const 9) (i32.const 32) (i32.const 19)))
    (call $log (i32.const 0) (i32.const 9))
    (i64.or (i64.shl (i64.const 128) (i64.const 32)) (i64.const 32))))
"#;
    wat::parse_str(wat).unwrap()
}

/// A guest that answers with the envelope it was handed, verbatim.
///
/// The reply is not a `{"status":..}` envelope, so the handler passes it through
/// as `200` and the test can parse the host's own envelope out of the body. That
/// is the only way to see what the guest actually received rather than what the
/// host meant to send.
fn echo_guest() -> Vec<u8> {
    let wat = r#"
(module
  (memory (export "memory") 1)
  (global $bump (mut i32) (i32.const 8192))
  (func (export "anth_alloc") (param $len i32) (result i32)
    (local $ptr i32)
    (local.set $ptr (global.get $bump))
    (global.set $bump (i32.add (global.get $bump) (local.get $len)))
    (local.get $ptr))
  (func (export "anth_free") (param $ptr i32) (param $len i32) (drop (local.get $ptr)) (drop (local.get $len)))
  (func (export "anth_abi_version") (result i32) (i32.const 1))
  (func (export "anth_handle") (param $ptr i32) (param $len i32) (result i64)
    (local $i i32)
    (block $done
      (loop $copy
        (br_if $done (i32.ge_u (local.get $i) (local.get $len)))
        (i32.store8
          (i32.add (i32.const 4096) (local.get $i))
          (i32.load8_u (i32.add (local.get $ptr) (local.get $i))))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $copy)))
    (i64.or (i64.shl (i64.const 4096) (i64.const 32)) (i64.extend_i32_u (local.get $len)))))
"#;
    wat::parse_str(wat).unwrap()
}

/// Installs a bundle by hand: no zip, no installer, because these tests are
/// about the request path, not the archive path.
fn install(root: &Path, id: &str, wasm: &[u8], api: &[(&str, &str)]) -> PluginGrant {
    let (grant, _) = install_with_manifest(root, id, wasm, api);
    grant
}

/// Same, and also returns the manifest text, for a test that wants to assert on
/// what it declared.
fn install_with_manifest(
    root: &Path,
    id: &str,
    wasm: &[u8],
    api: &[(&str, &str)],
) -> (PluginGrant, String) {
    let dir = root.join(id);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("plugin.wasm"), wasm).unwrap();

    let mut anchors = String::new();
    for (method, path) in api {
        anchors.push_str(&format!(
            "\n[[api]]\nmethod = \"{method}\"\npath = \"{path}\"\n"
        ));
    }
    let manifest = format!(
        r#"
[plugin]
id = "{id}"
version = "0.1.0"
name = "Anchored"
abi = 1

[wasm]
path = "plugin.wasm"
sha256 = "{}"

[[capability]]
id = "wg.status"
program = "wg"
argv = ["show", "{{interface}}"]

[capability.args.interface]
type = "ifname"
{anchors}
"#,
        sha_of(wasm)
    );
    fs::write(dir.join("plugin.toml"), &manifest).unwrap();

    let parsed = Manifest::parse(&manifest).unwrap();
    (PluginGrant::from_manifest(&parsed), manifest)
}

fn state(root: &Path, grant: PluginGrant) -> AppState {
    let mut config = Config::default();
    config.plugins.enabled = true;
    config.plugins.root = root.to_string_lossy().into_owned();
    config.plugins.grants = vec![grant];
    AppState::new(config)
}

async fn get(state: AppState, uri: &str) -> (StatusCode, String) {
    let response = anth_panel::api::router(state)
        .oneshot(HttpRequest::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

/// A `wg` stand-in on `PATH` that records its argv and prints a marker, so a test
/// can prove the spawn happened and with what arguments.
/// Serializes tests that point `PATH` at a fake program.
///
/// `set_var` is process-wide and the test harness runs these functions on
/// parallel threads, so two tests each setting `PATH` can see the other's
/// directory. The window is narrow — between `set_var` and the handler's spawn —
/// which makes it exactly the kind of flake that shows up once in a hundred CI
/// runs. The HTTP path builds its own `SystemRunner`, so a fake runner cannot be
/// injected; serializing is what is available.
fn path_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Points `PATH` at `bin` for as long as the guard lives, then restores it.
///
/// Holding the lock and the value together is what makes the comment honest:
/// the previous code restored nothing, so a later test in the same binary
/// inherited the fake `wg` by accident.
struct PathGuard {
    _lock: MutexGuard<'static, ()>,
    previous: Option<std::ffi::OsString>,
}

impl PathGuard {
    fn new(bin: &Path) -> Self {
        let lock = path_lock();
        let previous = std::env::var_os("PATH");
        // SAFETY: `_lock` serializes every mutation of `PATH` in this binary,
        // and it is held until `Drop` restores the original value.
        unsafe { std::env::set_var("PATH", format!("{}:/usr/bin:/bin", bin.display())) };
        Self {
            _lock: lock,
            previous,
        }
    }
}

impl Drop for PathGuard {
    fn drop(&mut self) {
        // SAFETY: the lock is still held here; `_lock` drops after this body.
        unsafe {
            match &self.previous {
                Some(value) => std::env::set_var("PATH", value),
                None => std::env::remove_var("PATH"),
            }
        }
    }
}

fn fake_wg(dir: &Path, stdout: &str) -> PathBuf {
    let bin = dir.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let script = bin.join("wg");
    let record = dir.join("argv.txt");
    let mut file = fs::File::create(&script).unwrap();
    writeln!(
        file,
        "#!/bin/sh\necho \"$@\" > '{}'\necho '{}'\n",
        record.display(),
        stdout
    )
    .unwrap();
    drop(file);
    let mut permissions = fs::metadata(&script).unwrap().permissions();
    use std::os::unix::fs::PermissionsExt;
    permissions.set_mode(0o755);
    fs::set_permissions(&script, permissions).unwrap();
    bin
}

#[test]
fn an_anchor_lands_under_its_plugin_prefix() {
    let anchor = ApiAnchor {
        method: "GET".to_string(),
        path: "/status".to_string(),
    };
    assert_eq!(
        anchor_route("org.anth.test", &anchor).unwrap(),
        "/api/plugins/org.anth.test/status"
    );

    let nested = ApiAnchor {
        method: "POST".to_string(),
        path: "/peers/one".to_string(),
    };
    assert_eq!(
        anchor_route("org.anth.test", &nested).unwrap(),
        "/api/plugins/org.anth.test/peers/one"
    );
}

#[test]
fn a_repeated_slash_collapses_instead_of_reaching_two_routes() {
    let anchor = ApiAnchor {
        method: "GET".to_string(),
        path: "//status".to_string(),
    };
    assert!(
        anchor_route("org.anth.test", &anchor).is_err(),
        "a leading double slash is refused outright"
    );

    let inner = ApiAnchor {
        method: "GET".to_string(),
        path: "/status//now".to_string(),
    };
    assert_eq!(
        anchor_route("org.anth.test", &inner).unwrap(),
        "/api/plugins/org.anth.test/status/now",
        "a doubled slash inside the path normalizes, it does not fork the route"
    );
}

#[test]
fn a_route_cannot_escape_its_namespace() {
    for path in ["/../api/auth/login", "/status/../../auth", "/./../x"] {
        let anchor = ApiAnchor {
            method: "GET".to_string(),
            path: path.to_string(),
        };
        let error = anchor_route("org.anth.test", &anchor).unwrap_err();
        assert!(
            error.detail.contains("namespace"),
            "`{path}` must be refused as escaping, got: {}",
            error.detail
        );
    }
}

#[test]
fn a_manifest_declaring_a_core_route_is_refused_at_parse() {
    let manifest = r#"
[plugin]
id = "org.anth.test"
version = "0.1.0"
name = "Test"
abi = 1

[wasm]
path = "plugin.wasm"
sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"

[[api]]
method = "POST"
path = "/../auth/login"
"#;
    let error = Manifest::parse(manifest).unwrap_err();
    assert!(error.detail.contains("namespace"), "{}", error.detail);
}

#[tokio::test]
async fn an_undeclared_route_still_reaches_the_guest() {
    // The manifest lists `/status`; the request asks for `/other`. It must reach
    // the guest, because the grant — not the manifest — is the boundary. A test
    // asserting a 404 here would be asserting that the untrusted file is the
    // router.
    let dir = unique_dir("undeclared");
    let wasm = routed_guest();
    let grant = install(&dir, "org.anth.test", &wasm, &[("GET", "/status")]);
    let bin = fake_wg(&dir, "wg0 up");
    let _path = PathGuard::new(&bin);

    let (status, body) = get(state(&dir, grant), "/api/plugins/org.anth.test/other").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains("anchored"), "{body}");

    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn the_guest_answers_with_the_envelope_it_was_given() {
    let dir = unique_dir("envelope");
    let wasm = routed_guest();
    let grant = install(&dir, "org.anth.test", &wasm, &[("GET", "/status")]);
    let bin = fake_wg(&dir, "wg0 up");
    let _path = PathGuard::new(&bin);

    let state = state(&dir, grant);
    let response = anth_panel::api::router(state)
        .oneshot(
            HttpRequest::builder()
                .uri("/api/plugins/org.anth.test/status?peer=wg1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let record = dir.join("argv.txt");
    let argv = fs::read_to_string(&record).unwrap();
    assert_eq!(
        argv.trim(),
        "show wg0",
        "the capability reached the program with the frozen template"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a_plugin_that_is_not_installed_is_not_found() {
    let dir = unique_dir("missing");
    let wasm = routed_guest();
    let grant = install(&dir, "org.anth.test", &wasm, &[("GET", "/status")]);

    let (status, body) = get(state(&dir, grant), "/api/plugins/org.anth.other/x").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert!(body.contains("no plugin"), "{body}");

    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn an_id_that_is_not_reverse_dns_never_reaches_a_directory_join() {
    let dir = unique_dir("badid");
    let wasm = routed_guest();
    let grant = install(&dir, "org.anth.test", &wasm, &[("GET", "/status")]);

    // `%2e%2e` decodes to `..`; the id validator refuses it before `plugin_dir`.
    // 400 rather than 404: the request is malformed, and saying so is more
    // useful to a plugin author than pretending the plugin is absent.
    let (status, body) = get(state(&dir, grant), "/api/plugins/%2e%2e/x").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn anchors_are_off_when_plugins_are_off() {
    let dir = unique_dir("disabled");
    let wasm = routed_guest();
    let grant = install(&dir, "org.anth.test", &wasm, &[("GET", "/status")]);

    // Build the state disabled from the start: the router reads the switch per
    // request, not at construction.
    let mut config = Config::default();
    config.plugins.enabled = false;
    config.plugins.root = dir.to_string_lossy().into_owned();
    config.plugins.grants = vec![grant];
    let state = AppState::new(config);

    let (status, body) = get(state, "/api/plugins/org.anth.test/status").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert!(body.contains("disabled"), "{body}");

    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn an_oversized_anchor_body_is_refused_before_the_guest() {
    let dir = unique_dir("bigbody");
    let wasm = routed_guest();
    let grant = install(&dir, "org.anth.test", &wasm, &[("GET", "/status")]);
    let state = state(&dir, grant);

    let response = anth_panel::api::router(state)
        .oneshot(
            HttpRequest::builder()
                .method("POST")
                .uri("/api/plugins/org.anth.test/status")
                .body(Body::from(vec![b'x'; MAX_ANCHOR_BODY + 1]))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), STATUS_FOR_BIG);

    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a_refused_capability_is_a_value_the_guest_can_answer_around() {
    let dir = unique_dir("denied");
    // A guest that asks for a capability the grant does not carry.
    let wat = r#"
(module
  (import "env" "anth_exec" (func $exec (param i32 i32 i32 i32) (result i64)))
  (import "env" "anth_log" (func $log (param i32 i32)))
  (memory (export "memory") 1)
  (func (export "anth_alloc") (param $len i32) (result i32) (i32.const 1024))
  (func (export "anth_free") (param $ptr i32) (param $len i32) (drop (local.get $ptr)) (drop (local.get $len)))
  (func (export "anth_abi_version") (result i32) (i32.const 1))
  (data (i32.const 0) "wg.delete")
  (data (i32.const 32) "{}")
  (data (i32.const 64) "{\"status\":200,\"body\":\"ran\"}")
  (func (export "anth_handle") (param $ptr i32) (param $len i32) (result i64)
    (drop (call $exec (i32.const 0) (i32.const 9) (i32.const 32) (i32.const 2)))
    (i64.or (i64.shl (i64.const 64) (i64.const 32)) (i64.const 27))))
"#;
    let wasm = wat::parse_str(wat).unwrap();
    let grant = install(&dir, "org.anth.test", &wasm, &[("GET", "/status")]);

    // `anth_exec` returns a refusal as a JSON value, not a trap, so the guest
    // can log it and answer anyway: the response is 200 and carries the guest's
    // own body. What must not happen is a spawn, so the absent argv file is the
    // assertion that matters.
    let (status, body) = get(state(&dir, grant), "/api/plugins/org.anth.test/status").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains("ran"), "{body}");
    assert!(
        !dir.join("argv.txt").exists(),
        "a capability outside the grant must not reach the program"
    );

    let _ = fs::remove_dir_all(&dir);
}

/// The anchors are behind the same layer as every other `/api` route. This is
/// the property worth a test of its own: `/api/plugins/<id>/...` is a wildcard,
/// so a middleware that special-cased the exact plugin routes would silently
/// leave every anchor open.
#[tokio::test]
async fn an_anchor_requires_a_token_like_every_other_route() {
    let dir = unique_dir("auth");
    let wasm = routed_guest();
    let grant = install(&dir, "org.anth.test", &wasm, &[("GET", "/status")]);

    let mut config = Config {
        auth: anth_panel::config::AuthConfig {
            enabled: true,
            username: "admin".to_string(),
            password_hash: anth_panel::auth::password::hash_password_with_iterations(
                "hunter2",
                anth_panel::auth::password::DEV_ITERATIONS,
            )
            .unwrap(),
            jwt_secret: "test-secret".to_string(),
            access_token_secs: 900,
            refresh_token_secs: 3600,
            iterations: anth_panel::auth::password::DEV_ITERATIONS,
            cookie_secure: false,
            bootstrap_password: None,
        },
        ..Config::default()
    };
    config.plugins.enabled = true;
    config.plugins.root = dir.to_string_lossy().into_owned();
    config.plugins.grants = vec![grant];

    let (status, body) = get(AppState::new(config), "/api/plugins/org.anth.test/status").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");

    let _ = fs::remove_dir_all(&dir);
}

/// The guest must receive `/status`, which is how its manifest spells the anchor,
/// however the request spelled it.
///
/// axum strips the leading slash from a `{*path}` capture, so the ordinary
/// request arrives as `status` while `//status` arrives as `/status`. Before this
/// was normalized, an anchor that dispatched on its declared paths matched only
/// the doubled-slash spelling — the one nobody sends.
#[tokio::test]
async fn every_spelling_of_a_route_reaches_the_guest_as_the_declared_path() {
    let dir = unique_dir("canonical");
    let wasm = echo_guest();
    let grant = install(&dir, "org.anth.test", &wasm, &[("GET", "/status")]);
    let state = state(&dir, grant);

    for (uri, expected) in [
        ("/api/plugins/org.anth.test/status", "/status"),
        ("/api/plugins/org.anth.test//status", "/status"),
        ("/api/plugins/org.anth.test/status/", "/status"),
        ("/api/plugins/org.anth.test/wg/peers", "/wg/peers"),
    ] {
        let (status, body) = get(state.clone(), uri).await;
        assert_eq!(status, StatusCode::OK, "{uri}: {body}");
        let envelope: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(envelope["kind"], "http", "{uri}: {body}");
        assert_eq!(envelope["path"], expected, "{uri}: {body}");
    }

    let _ = fs::remove_dir_all(&dir);
}

/// A `plugin.toml` rewritten to escape its namespace is a broken bundle, not a
/// forbidden caller. The documented answer is `422`; `403` would tell a client
/// holding a valid token that it may not do something it never asked for.
#[tokio::test]
async fn a_rewritten_manifest_answers_422_not_403() {
    let dir = unique_dir("rewritten");
    let wasm = routed_guest();
    let grant = install(&dir, "org.anth.test", &wasm, &[("GET", "/status")]);

    let manifest = dir.join("org.anth.test/plugin.toml");
    let mut text = fs::read_to_string(&manifest).unwrap();
    text.push_str("\n[[api]]\nmethod = \"POST\"\npath = \"/../auth/login\"\n");
    fs::write(&manifest, text).unwrap();

    let (status, body) = get(state(&dir, grant), "/api/plugins/org.anth.test/status").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body.contains("namespace"), "{body}");

    let _ = fs::remove_dir_all(&dir);
}

const STATUS_FOR_BIG: StatusCode = StatusCode::PAYLOAD_TOO_LARGE;

/// A manifest rewritten on disk after install is caught by `load`, because `load`
/// parses it. Any hash or grant check happens after that, so a bundle whose
/// `plugin.toml` now names a core route never reaches the module.
#[test]
fn a_rewritten_manifest_is_refused_before_the_module_is_read() {
    let dir = unique_dir("recheck");
    let wasm = routed_guest();
    let (grant, _) = install_with_manifest(&dir, "org.anth.test", &wasm, &[]);

    let manifest = dir.join("org.anth.test/plugin.toml");
    let mut text = fs::read_to_string(&manifest).unwrap();
    // Appended, not spliced: a table header inserted mid-file would close
    // `[plugin]` and leave `abi` in the wrong table.
    text.push_str("\n[[api]]\nmethod = \"POST\"\npath = \"/../auth/login\"\n");
    fs::write(&manifest, text).unwrap();

    let error = anth_panel::plugins::bundle::load(&dir.join("org.anth.test"), &grant).unwrap_err();
    assert!(error.detail.contains("namespace"), "{}", error.detail);

    let _ = fs::remove_dir_all(&dir);
}
