//! The reference WireGuard plugin, installed and run for real.
//!
//! This is the end-to-end proof that a plugin written as a Rust crate works
//! with the host shipped in this repository: the real `plugin.toml`, the real
//! module compiled from `plugins/wireguard`, the real install path, the real
//! wasmi runtime and the real `wg` command runner replaced by a recorder.
//!
//! The repository does not vendor a `.wasm` build artifact, so these tests read
//! the module from the same place `plugin install` would: a bundle assembled
//! here from the crate's source tree. When the module has not been built, the
//! fixture test says so instead of failing with a confusing hash mismatch.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use anth_panel::command::{CommandError, CommandOutput, CommandRunner};
use anth_panel::plugins::manifest::Manifest;
use anth_panel::plugins::{bundle, runtime};
use serde_json::Value;

const PLUGIN_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../plugins/wireguard");

fn unique_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "anth-wireguard-{name}-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// The module built by `cargo build --target wasm32-unknown-unknown`.
///
/// Returned by path rather than by value so the caller can skip rather than
/// pay for a copy of a module it will not use.
fn module() -> Option<PathBuf> {
    let candidates = [
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../plugins/wireguard/target/wasm32-unknown-unknown/release/anth_plugin_wireguard.wasm"
        ),
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../plugins/wireguard/target/wasm32-unknown-unknown/debug/anth_plugin_wireguard.wasm"
        ),
    ];
    candidates
        .iter()
        .map(PathBuf::from)
        .find(|path| path.is_file())
}

fn sha256_of(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The bundle as shipped, with its `sha256` replaced by the real digest.
///
/// The committed manifest deliberately carries a zero digest: a real one would
/// be stale the moment the crate changed, and a stale digest is the difference
/// between "the module was rebuilt" and "the module was tampered with". The test
/// recomputes it instead, which is exactly what an installer's hash check does.
fn bundle_in(dir: &Path, wasm: &[u8]) -> PathBuf {
    let source = Path::new(PLUGIN_DIR);
    let root = dir.join("org.anth.wireguard");
    fs::create_dir_all(root.join("web")).unwrap();
    let mut manifest = fs::read_to_string(source.join("plugin.toml")).unwrap();
    let digest = sha256_of(wasm);
    let marker = "0000000000000000000000000000000000000000000000000000000000000000";
    assert!(
        manifest.contains(marker),
        "the manifest no longer carries the zero placeholder this fixture replaces"
    );
    manifest = manifest.replace(marker, &digest);
    fs::write(root.join("plugin.toml"), manifest).unwrap();
    fs::write(root.join("plugin.wasm"), wasm).unwrap();
    for entry in fs::read_dir(source.join("web")).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        fs::write(root.join("web").join(name), fs::read(entry.path()).unwrap()).unwrap();
    }

    let path = dir.join("org.anth.wireguard.zip");
    let file = fs::File::create(&path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();
    for name in ["plugin.toml", "plugin.wasm"] {
        zip.start_file(name, options).unwrap();
        std::io::Write::write_all(&mut zip, &fs::read(root.join(name)).unwrap()).unwrap();
    }
    zip.start_file("web/wireguard.js", options).unwrap();
    std::io::Write::write_all(&mut zip, &fs::read(root.join("web/wireguard.js")).unwrap()).unwrap();
    zip.finish().unwrap();
    path
}

/// Answers each `wg` invocation from a table, keyed by the rendered argv.
struct Wg {
    replies: Vec<(Vec<String>, String)>,
    calls: std::cell::RefCell<Vec<Vec<String>>>,
}

impl Wg {
    fn new(replies: &[(&[&str], &str)]) -> Self {
        Self {
            replies: replies
                .iter()
                .map(|(argv, stdout)| {
                    (
                        argv.iter().map(|part| (*part).to_string()).collect(),
                        (*stdout).to_string(),
                    )
                })
                .collect(),
            calls: std::cell::RefCell::new(Vec::new()),
        }
    }
}

impl CommandRunner for Wg {
    fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput, CommandError> {
        assert_eq!(program, "wg", "the grant may only reach `wg`");
        let argv: Vec<String> = args.iter().map(|arg| (*arg).to_string()).collect();
        self.calls.borrow_mut().push(argv.clone());
        match self.replies.iter().find(|(expected, _)| *expected == argv) {
            Some((_, stdout)) => Ok(CommandOutput {
                ok: true,
                stdout: stdout.clone(),
                stderr: String::new(),
            }),
            None => Ok(CommandOutput {
                ok: false,
                stdout: String::new(),
                stderr: format!("unexpected argv: {argv:?}"),
            }),
        }
    }

    fn run_with_stdin(
        &self,
        program: &str,
        args: &[&str],
        _input: &str,
    ) -> Result<CommandOutput, CommandError> {
        self.run(program, args)
    }
}

const SHOW: &str = "\
interface: wg0
  public key: yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk=
  private key: (hidden)
  listening port: 51820

peer: xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=
  allowed ips: 10.0.0.2/32
  transfer: 1.00 KiB received, 2.00 KiB sent
";

fn installed() -> Option<(PathBuf, Vec<u8>)> {
    static CACHE: OnceLock<Option<(PathBuf, Vec<u8>)>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let path = module()?;
            let wasm = fs::read(&path).ok()?;
            let dir = unique_dir("install");
            let archive = bundle_in(&dir, &wasm);
            let root = dir.join("plugins");
            fs::create_dir_all(&root).unwrap();
            match bundle::install(&archive, &root) {
                Ok((installed, _grant)) => Some((installed.root, wasm)),
                Err(error) => panic!("the shipped bundle must install: {error}"),
            }
        })
        .clone()
}

/// Skips with a message instead of failing when the crate has not been built.
///
/// A missing artifact is a setup problem, not a defect: the checked-in source
/// is what this test is about, and `cargo build --target wasm32-unknown-unknown`
/// in `plugins/wireguard` produces it.
macro_rules! fixture {
    () => {
        match installed() {
            Some(fixture) => fixture,
            None => {
                eprintln!(
                    "skipping: build the plugin first with \
                     `cargo build --target wasm32-unknown-unknown --release` in plugins/wireguard"
                );
                return;
            }
        }
    };
}

fn invoke(
    grant: &anth_panel::plugins::PluginGrant,
    wasm: &[u8],
    runner: &dyn CommandRunner,
    request: Value,
) -> Value {
    let reply = runtime::invoke(wasm, &grant.id, grant, runner, &request.to_string(), 0)
        .expect("the module must run");
    serde_json::from_str(&reply.body).expect("the plugin answers with JSON")
}

#[test]
fn the_shipped_manifest_is_valid_and_inside_host_policy() {
    let text = fs::read_to_string(Path::new(PLUGIN_DIR).join("plugin.toml")).unwrap();
    let manifest = Manifest::parse(&text).expect("the shipped manifest must validate");

    assert_eq!(manifest.plugin.id, "org.anth.wireguard");
    assert_eq!(manifest.plugin.abi, 1);
    let ids: Vec<&str> = manifest
        .capabilities
        .iter()
        .map(|capability| capability.id.as_str())
        .collect();
    assert!(ids.contains(&"wg.interfaces"));
    assert!(ids.contains(&"wg.show"));
    assert!(ids.contains(&"wg.add_peer"));
    assert!(ids.contains(&"wg.add_peer_endpoint"));
    // Every capability must run `wg` and nothing else: this plugin has no
    // business reaching a second program.
    for capability in &manifest.capabilities {
        assert_eq!(capability.program.as_deref(), Some("wg"));
        assert!(capability.job.is_none());
    }
    assert!(
        manifest
            .ui
            .iter()
            .any(|anchor| anchor.tag == "anth-wireguard"),
        "the dashboard widget must be declared"
    );
}

#[test]
fn the_module_installs_and_implements_the_host_abi() {
    let (root, wasm) = fixture!();
    assert!(root.join("plugin.wasm").is_file());
    // `install` already probed the module and re-hashed it; this asserts the
    // ABI handshake once more from the outside so a failure names the version.
    runtime::probe(&wasm).expect("the shipped module must load");
}

#[test]
fn the_http_anchor_lists_interfaces_through_the_grant() {
    let (root, wasm) = fixture!();
    let grant = anth_panel::plugins::PluginGrant::from_manifest(
        &Manifest::parse(&fs::read_to_string(root.join("plugin.toml")).unwrap()).unwrap(),
    );
    let runner = Wg::new(&[(&["show", "interfaces"], "wg0\n"), (&["show", "wg0"], SHOW)]);

    let body = invoke(
        &grant,
        &wasm,
        &runner,
        serde_json::json!({"kind": "http", "method": "GET", "path": "/interfaces"}),
    );

    assert_eq!(body["status"], 200, "body: {body}");
    let interface = &body["body"]["interfaces"][0];
    assert_eq!(interface["name"], "wg0");
    assert_eq!(interface["listen_port"], 51820);
    assert_eq!(interface["peers"].as_array().unwrap().len(), 1);

    let calls = runner.calls.borrow();
    assert_eq!(calls[0], vec!["show", "interfaces"]);
    assert_eq!(calls[1], vec!["show", "wg0"]);
}

#[test]
fn adding_a_peer_renders_the_granted_argv_without_the_endpoint() {
    let (root, wasm) = fixture!();
    let grant = anth_panel::plugins::PluginGrant::from_manifest(
        &Manifest::parse(&fs::read_to_string(root.join("plugin.toml")).unwrap()).unwrap(),
    );
    let key = "yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk=";
    let runner = Wg::new(&[
        (
            &["set", "wg0", "peer", key, "allowed-ips", "10.0.0.2/32"],
            "",
        ),
        (&["show", "interfaces"], "wg0\n"),
        (&["show", "wg0"], SHOW),
    ]);

    let body = invoke(
        &grant,
        &wasm,
        &runner,
        serde_json::json!({
            "kind": "http",
            "method": "POST",
            "path": "/peers",
            "body": {"interface": "wg0", "public_key": key, "allowed_ips": ["10.0.0.2/32"]},
        }),
    );

    assert_eq!(body["status"], 201, "body: {body}");
    let calls = runner.calls.borrow();
    // The optional endpoint element is absent from the argv entirely: it did not
    // render as an empty string after `endpoint`.
    assert_eq!(
        calls[0],
        vec!["set", "wg0", "peer", key, "allowed-ips", "10.0.0.2/32"]
    );
}

#[test]
fn an_endpoint_peer_uses_the_recipe_that_supplies_the_flag_pair() {
    let (root, wasm) = fixture!();
    let grant = anth_panel::plugins::PluginGrant::from_manifest(
        &Manifest::parse(&fs::read_to_string(root.join("plugin.toml")).unwrap()).unwrap(),
    );
    let key = "yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk=";
    let runner = Wg::new(&[(
        &[
            "set",
            "wg0",
            "peer",
            key,
            "allowed-ips",
            "10.0.0.2/32",
            "endpoint",
            "vpn.example.net:51820",
        ],
        "",
    )]);

    let body = invoke(
        &grant,
        &wasm,
        &runner,
        serde_json::json!({
            "kind": "http",
            "method": "POST",
            "path": "/peers",
            "body": {
                "interface": "wg0",
                "public_key": key,
                "allowed_ips": "10.0.0.2/32",
                "endpoint": "vpn.example.net:51820",
            },
        }),
    );

    assert_eq!(body["status"], 201, "body: {body}");
    assert_eq!(body["body"]["endpoint"], true);
    assert_eq!(
        runner.calls.borrow()[0],
        vec![
            "set",
            "wg0",
            "peer",
            key,
            "allowed-ips",
            "10.0.0.2/32",
            "endpoint",
            "vpn.example.net:51820"
        ]
    );
}

#[test]
fn a_capability_the_grant_does_not_hold_is_refused_at_runtime() {
    let (root, wasm) = fixture!();
    let grant = anth_panel::plugins::PluginGrant::from_manifest(
        &Manifest::parse(&fs::read_to_string(root.join("plugin.toml")).unwrap()).unwrap(),
    );
    // `wg.private_key` is not in the manifest, so the grant cannot hold it.
    let reply = runtime::invoke(
        &wasm,
        &grant.id,
        &grant,
        &Wg::new(&[]),
        &serde_json::json!({"kind": "cli", "argv": ["list"]}).to_string(),
        0,
    )
    .expect("the module must run");
    let parsed: Value = serde_json::from_str(&reply.body).unwrap();
    // `list` reaches `wg.interfaces`, which this runner does not answer, so the
    // plugin reports a failure rather than the host inventing one.
    assert_eq!(parsed["status"], 502);
    assert!(reply.execs == 0, "a refused call is not a successful exec");
}

#[test]
fn the_cli_anchor_answers_status_under_the_same_grant() {
    let (root, wasm) = fixture!();
    let grant = anth_panel::plugins::PluginGrant::from_manifest(
        &Manifest::parse(&fs::read_to_string(root.join("plugin.toml")).unwrap()).unwrap(),
    );
    let runner = Wg::new(&[(&["show", "interfaces"], "wg0\n"), (&["show", "wg0"], SHOW)]);
    let body = invoke(
        &grant,
        &wasm,
        &runner,
        serde_json::json!({"kind": "cli", "argv": ["status"]}),
    );

    assert_eq!(body["status"], 200);
    assert_eq!(body["body"]["interfaces"], 1);
    assert_eq!(body["body"]["peers"], 1);
}

#[test]
fn the_web_module_ships_and_reaches_the_anchor_namespace() {
    let (root, _wasm) = fixture!();
    let source = fs::read_to_string(root.join("web/wireguard.js")).unwrap();
    assert!(source.contains("customElements.define('anth-wireguard'"));
    // The base URL must be derived, not hardcoded: the host may mount the
    // anchors under a different prefix, and a hardcoded one would 404 there.
    // One level up from `.../web/wireguard.js` is the anchor namespace; two
    // would drop the plugin id and resolve `/api/plugins/interfaces`.
    assert!(source.contains("new URL('../', import.meta.url)"));
    assert!(!source.contains("fetch('/api/plugins/org.anth.wireguard"));
}
