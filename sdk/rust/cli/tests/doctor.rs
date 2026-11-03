use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("anth-doctor-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir");
    dir
}

/// A stand-in for `anth-engine --check`: records its arguments and exits with
/// the code the test asked for. It keeps the test independent of a built engine.
fn fake_engine(dir: &Path, exit: i32) -> PathBuf {
    let args = dir.join("args");
    let script = dir.join("fake-engine");
    let source = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{args}'\necho 'ok: fake engine'\nexit {exit}\n",
        args = args.display()
    );
    fs::write(&script, source).expect("write");
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).expect("chmod");
    script
}

fn doctor(engine: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_anth"))
        .args(arguments)
        .env("ANTH_ENGINE", engine)
        .output()
        .expect("run anth")
}

#[test]
fn doctor_forwards_the_flags_and_returns_the_engine_result() {
    let dir = scratch("forward");
    let engine = fake_engine(&dir, 0);
    let output = doctor(
        &engine,
        &["doctor", "--config", "/etc/anth.toml", "--debug"],
    );
    assert!(output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("ok: fake engine"),
        "the engine's output must reach the terminal"
    );
    let recorded = fs::read_to_string(dir.join("args")).expect("args");
    assert_eq!(
        recorded.lines().collect::<Vec<_>>(),
        ["--check", "--config", "/etc/anth.toml", "--debug"]
    );
}

#[test]
fn doctor_passes_the_engine_exit_code_through() {
    let dir = scratch("exit");
    let engine = fake_engine(&dir, 4);
    let output = doctor(&engine, &["doctor"]);
    assert_eq!(output.status.code(), Some(4));
}

#[test]
fn doctor_without_an_engine_binary_is_not_found() {
    let output = doctor(Path::new("/nonexistent/engine"), &["doctor"]);
    assert_eq!(output.status.code(), Some(4));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ANTH_ENGINE"), "{stderr}");
}

#[test]
fn help_and_version_do_not_need_an_engine() {
    let output = Command::new(env!("CARGO_BIN_EXE_anth"))
        .arg("--help")
        .env_remove("ANTH_ENGINE")
        .output()
        .expect("run anth");
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("doctor"));

    let output = Command::new(env!("CARGO_BIN_EXE_anth"))
        .arg("--version")
        .env_remove("ANTH_ENGINE")
        .output()
        .expect("run anth");
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).starts_with("anth "));
}
