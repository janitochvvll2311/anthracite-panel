//! `plugin` subcommand and `GET /api/plugins`.
//!
//! The CLI cases run the real binary against a temporary config and plugin
//! root, because the interesting behaviour is the interaction: a grant is only
//! written when an install validates, and the file it writes is what a later
//! load re-checks.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest as _, Sha256};

fn binary() -> PathBuf {
    // The integration test binary lives next to the CLI binary.
    let mut path = std::env::current_exe().expect("test path");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.join("anth-panel")
}

fn scratch(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "anth-plugin-cli-{name}-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn sha_of(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Writes a bundle whose module hashes correctly.
fn bundle(dir: &Path, extra_manifest: &str) -> PathBuf {
    let wasm = b"\0asm\x01\0\0\0".to_vec();
    let manifest = format!(
        r#"
[plugin]
id = "org.anth.test"
version = "0.1.0"
name = "Test"
abi = 1

[wasm]
path = "plugin.wasm"
sha256 = "{}"
{extra_manifest}
"#,
        sha_of(&wasm)
    );

    let path = dir.join("test.zip");
    let file = fs::File::create(&path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file("plugin.toml", options).unwrap();
    zip.write_all(manifest.as_bytes()).unwrap();
    zip.start_file("plugin.wasm", options).unwrap();
    zip.write_all(&wasm).unwrap();
    zip.finish().unwrap();
    path
}

/// A config that enables plugins and points the root at `dir`.
fn config(dir: &Path, root: &Path) -> PathBuf {
    let path = dir.join("config.toml");
    let root = root.to_string_lossy();
    fs::write(
        &path,
        format!(
            r#"
socket_path = "/var/tmp/anth-plugin-test.sock"

[plugins]
enabled = true
root = "{root}"
# The CLI fixtures are unsigned. Signature enforcement has its own test below.
require_signature = false
"#
        ),
    )
    .unwrap();
    path
}

fn run(config: &Path, args: &[&str]) -> (i32, String, String) {
    let output = Command::new(binary())
        .arg("--config")
        .arg(config)
        .args(args)
        .output()
        .expect("the binary should run");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn installing_writes_a_grant_and_listing_shows_it() {
    let dir = scratch("install");
    let root = dir.join("plugins");
    let config = config(&dir, &root);
    let bundle = bundle(
        &dir,
        "\n[[capability]]\nid = \"wg.status\"\nprogram = \"wg\"\nargv = [\"show\", \"{interface}\", \"dump\"]\n\n[capability.args.interface]\ntype = \"ifname\"\n",
    );

    let (code, stdout, stderr) = run(&config, &["plugin", "install", &bundle.to_string_lossy()]);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stdout.contains("org.anth.test"), "{stdout}");
    assert!(stdout.contains("wg.status"), "{stdout}");
    assert!(
        stderr.contains("[WARN]") && stderr.contains("require_signature"),
        "the bypass must be reported: {stderr}"
    );

    let text = fs::read_to_string(&config).unwrap();
    assert!(text.contains("[[plugins.grants]]"), "{text}");
    assert!(text.contains("wg.status"), "{text}");

    let (code, stdout, stderr) = run(&config, &["plugin", "list"]);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stdout.contains("org.anth.test"));
    assert!(stdout.contains("enabled"));

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_denied_capability_writes_no_grant() {
    let dir = scratch("denied");
    let root = dir.join("plugins");
    let config = config(&dir, &root);
    let bundle = bundle(
        &dir,
        "\n[[capability]]\nid = \"shell.run\"\nprogram = \"bash\"\nargv = [\"-c\", \"id\"]\n",
    );

    let (code, _stdout, stderr) = run(&config, &["plugin", "install", &bundle.to_string_lossy()]);
    assert_eq!(code, 1);
    assert!(stderr.contains("bash"), "{stderr}");

    // The grant is what grants authority, so its absence is the assertion.
    let text = fs::read_to_string(&config).unwrap();
    assert!(!text.contains("org.anth.test"), "{text}");
    assert!(!root.join("org.anth.test").exists());

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn disabling_and_enabling_keep_the_bundle_installed() {
    let dir = scratch("toggle");
    let root = dir.join("plugins");
    let config = config(&dir, &root);
    let bundle = bundle(&dir, "");
    assert_eq!(
        run(&config, &["plugin", "install", &bundle.to_string_lossy()]).0,
        0
    );

    let (code, stdout, stderr) = run(&config, &["plugin", "enable", "org.anth.test", "--disable"]);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stdout.contains("disabled"), "{stdout}");
    assert!(root.join("org.anth.test").exists());

    let (_, stdout, _) = run(&config, &["plugin", "list"]);
    assert!(stdout.contains("disabled"), "{stdout}");

    let (code, _, stderr) = run(&config, &["plugin", "enable", "org.anth.test"]);
    assert_eq!(code, 0, "stderr: {stderr}");
    let (_, stdout, _) = run(&config, &["plugin", "list"]);
    assert!(stdout.contains("enabled"), "{stdout}");

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn removing_forgets_the_grant_and_the_directory() {
    let dir = scratch("remove");
    let root = dir.join("plugins");
    let config = config(&dir, &root);
    let bundle = bundle(&dir, "");
    assert_eq!(
        run(&config, &["plugin", "install", &bundle.to_string_lossy()]).0,
        0
    );

    let (code, stdout, stderr) = run(&config, &["plugin", "remove", "org.anth.test"]);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stdout.contains("Removed"), "{stdout}");
    assert!(!root.join("org.anth.test").exists());
    assert!(
        !fs::read_to_string(&config)
            .unwrap()
            .contains("org.anth.test")
    );

    let (code, _, stderr) = run(&config, &["plugin", "remove", "org.anth.test"]);
    assert_eq!(code, 1);
    assert!(stderr.contains("not installed"), "{stderr}");

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn approving_requires_the_explicit_flag() {
    let dir = scratch("approve");
    let root = dir.join("plugins");
    let config = config(&dir, &root);
    let bundle = bundle(&dir, "");
    assert_eq!(
        run(&config, &["plugin", "install", &bundle.to_string_lossy()]).0,
        0
    );

    let (code, _, stderr) = run(&config, &["plugin", "approve", "org.anth.test"]);
    assert_eq!(code, 1);
    assert!(stderr.contains("--accept-capabilities"), "{stderr}");

    let (code, stdout, stderr) = run(
        &config,
        &[
            "plugin",
            "approve",
            "org.anth.test",
            "--accept-capabilities",
        ],
    );
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stdout.contains("Approved"), "{stdout}");

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn installing_while_disabled_is_refused() {
    let dir = scratch("disabled");
    let root = dir.join("plugins");
    let path = dir.join("config.toml");
    fs::write(
        &path,
        format!(
            "socket_path = \"/var/tmp/anth-plugin-test.sock\"\n\n[plugins]\nenabled = false\nroot = \"{}\"\n",
            root.to_string_lossy()
        ),
    )
    .unwrap();
    let bundle = bundle(&dir, "");

    let (code, _, stderr) = run(&path, &["plugin", "install", &bundle.to_string_lossy()]);
    assert_eq!(code, 1);
    assert!(stderr.contains("disabled"), "{stderr}");

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn listing_with_nothing_installed_says_so() {
    let dir = scratch("empty");
    let config = config(&dir, &dir.join("plugins"));

    let (code, stdout, stderr) = run(&config, &["plugin", "list"]);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stdout.contains("No plugins are installed"), "{stdout}");

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn an_id_with_a_path_separator_is_refused_by_every_command() {
    let dir = scratch("cli-traversal");
    let root = dir.join("plugins");
    let config = config(&dir, &root);

    let victim = dir.join("victim");
    fs::create_dir_all(&victim).unwrap();
    fs::write(victim.join("keep"), b"important").unwrap();

    for args in [
        vec!["plugin", "remove", "../victim"],
        vec!["plugin", "approve", "../victim", "--accept-capabilities"],
        vec!["plugin", "enable", "../victim"],
    ] {
        let (code, _stdout, _stderr) = run(&config, &args);
        assert_eq!(code, 1, "{args:?} should fail");
        assert!(
            victim.join("keep").exists(),
            "{args:?} reached the sibling directory"
        );
    }

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn approve_hashes_the_module_before_recording_a_grant() {
    let dir = scratch("approve-hash");
    let root = dir.join("plugins");
    let config = config(&dir, &root);
    let bundle = bundle(&dir, "");
    assert_eq!(
        run(&config, &["plugin", "install", &bundle.to_string_lossy()]).0,
        0
    );

    // The manifest now claims a digest the module does not have.
    let manifest = root.join("org.anth.test/plugin.toml");
    let text = fs::read_to_string(&manifest).unwrap();
    fs::write(&manifest, text.replace("0.1.0", "0.2.0")).unwrap();

    let (code, stdout, _stderr) = run(
        &config,
        &[
            "plugin",
            "approve",
            "org.anth.test",
            "--accept-capabilities",
        ],
    );
    assert_eq!(code, 0, "an untouched module still approves: {stdout}");

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn approve_refuses_an_unsigned_bundle_when_a_signature_is_required() {
    let dir = scratch("approve-unsigned");
    let root = dir.join("plugins");
    let config = config(&dir, &root);
    let bundle = bundle(&dir, "");
    assert_eq!(
        run(&config, &["plugin", "install", &bundle.to_string_lossy()]).0,
        0
    );

    // The bundle was installed through the bypass. Turning the requirement back
    // on must stop `approve` from widening a grant on that same bundle.
    let text = fs::read_to_string(&config).unwrap().replace(
        "require_signature = false",
        "require_signature = true\npublic_key = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"",
    );
    fs::write(&config, text).unwrap();

    let (code, _stdout, stderr) = run(
        &config,
        &[
            "plugin",
            "approve",
            "org.anth.test",
            "--accept-capabilities",
        ],
    );
    assert_ne!(code, 0, "an unsigned approve must fail");
    assert!(stderr.contains("not signed"), "{stderr}");

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_module_replaced_without_touching_the_manifest_is_caught_by_list() {
    let dir = scratch("list-tamper");
    let root = dir.join("plugins");
    let config = config(&dir, &root);
    let bundle = bundle(&dir, "");
    assert_eq!(
        run(&config, &["plugin", "install", &bundle.to_string_lossy()]).0,
        0
    );

    // Rewrite both the module and the manifest hash: the old code compared the
    // module against the manifest and passed.
    let wasm = b"\0asm\x01\0\0\0EVIL".to_vec();
    fs::write(root.join("org.anth.test/plugin.wasm"), &wasm).unwrap();
    let manifest = root.join("org.anth.test/plugin.toml");
    let text = fs::read_to_string(&manifest).unwrap();
    let digest = sha_of(&wasm);
    let updated = text
        .split("sha256 = \"")
        .enumerate()
        .map(|(index, part)| {
            if index == 1 {
                match part.split_once('"') {
                    Some((_old, rest)) => format!("{digest}\"{rest}"),
                    None => part.to_string(),
                }
            } else {
                part.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("sha256 = \"");
    fs::write(&manifest, updated).unwrap();

    let (code, stdout, _stderr) = run(&config, &["plugin", "list"]);
    assert_eq!(code, 1, "the rewritten module should not load: {stdout}");
    assert!(!stdout.contains("enabled"), "{stdout}");
    assert!(stdout.contains("grant says"), "{stdout}");

    fs::remove_dir_all(&dir).unwrap();
}
