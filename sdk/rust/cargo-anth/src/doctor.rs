//! `anth app doctor`: the local toolchain the build and signing steps use.

use std::path::Path;
use std::process::Command;

use anyhow::{bail, Result};

pub fn run() -> Result<()> {
    let cargo = tool("CARGO", "cargo");
    let rustc = tool("RUSTC", "rustc");
    let wasm_tools = tool("WASM_TOOLS", "wasm-tools");
    let bun = tool("BUN", "bun");
    let signer = tool("ANTH_SIGN", "anth-sign");
    let conformance = tool("ANTH_CONFORMANCE", "anth-conformance");

    let target = "wasm32-unknown-unknown";
    let target_ok = rustc
        .as_deref()
        .is_some_and(|rustc| target_installed(rustc, target));
    let mut missing = Vec::new();

    report("cargo", cargo.as_deref(), true, &mut missing);
    report("rustc", rustc.as_deref(), true, &mut missing);
    report(
        &format!("target {target}"),
        target_ok.then_some(target),
        true,
        &mut missing,
    );
    report("wasm-tools", wasm_tools.as_deref(), true, &mut missing);
    report("bun", bun.as_deref(), false, &mut missing);
    report("anth-sign", signer.as_deref(), false, &mut missing);
    report(
        "anth-conformance",
        conformance.as_deref(),
        false,
        &mut missing,
    );

    if missing.is_empty() {
        println!("doctor: the app toolchain is ready");
        Ok(())
    } else {
        bail!("missing required tools: {}", missing.join(", "))
    }
}

fn report(name: &str, path: Option<&str>, required: bool, missing: &mut Vec<String>) {
    match (path, required) {
        (Some(path), _) if path == name => println!("ok      {name}"),
        (Some("wasm32-unknown-unknown"), _) => println!("ok      {name}"),
        (Some(path), _) => println!("ok      {name} ({path})"),
        (None, true) => {
            println!("missing {name}");
            missing.push(name.to_string());
        }
        (None, false) => println!("optional {name}"),
    }
}

fn tool(env: &str, name: &str) -> Option<String> {
    if let Ok(value) = std::env::var(env) {
        if !value.is_empty() {
            return Some(value);
        }
    }
    if let Some(path) = find_in_path(name) {
        return Some(path);
    }
    let fallback = format!("/tools/cargo/bin/{name}");
    if Path::new(&fallback).is_file() {
        return Some(fallback);
    }
    None
}

fn find_in_path(name: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate.to_string_lossy().into_owned());
        }
    }
    None
}

fn target_installed(rustc: &str, target: &str) -> bool {
    Command::new(rustc)
        .arg("--print")
        .arg("target-libdir")
        .arg("--target")
        .arg(target)
        .output()
        .is_ok_and(|output| output.status.success())
}
