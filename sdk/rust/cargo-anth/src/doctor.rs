//! `anth app doctor`: the local toolchain the build and signing steps use.

use std::path::Path;
use std::process::Command;

use anyhow::{bail, Result};

use crate::project::find_tool;

pub fn run() -> Result<()> {
    let cargo = find_tool("CARGO", "cargo");
    let rustc = find_tool("RUSTC", "rustc");
    let wasm_tools = find_tool("WASM_TOOLS", "wasm-tools");
    let bun = find_tool("BUN", "bun");
    let signer = find_tool("ANTH_SIGN", "anth-sign");
    let conformance = find_tool("ANTH_CONFORMANCE", "anth-conformance");

    let target = "wasm32-unknown-unknown";
    let target_ok = rustc
        .as_deref()
        .is_some_and(|rustc| target_installed(rustc, target));
    let mut missing = Vec::new();

    report("cargo", cargo.as_deref(), true, &mut missing);
    report("rustc", rustc.as_deref(), true, &mut missing);
    report(
        &format!("target {target}"),
        target_ok.then_some(""),
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
        (Some(""), _) => println!("ok      {name}"),
        (Some(path), _) => println!("ok      {name} ({path})"),
        (None, true) => {
            println!("missing {name}");
            missing.push(name.to_string());
        }
        (None, false) => println!("optional {name}"),
    }
}

/// Rustup is authoritative; the sysroot check keeps a non-rustup install honest.
fn target_installed(rustc: &str, target: &str) -> bool {
    if let Some(rustup) = find_tool("RUSTUP", "rustup") {
        if let Ok(output) = Command::new(rustup)
            .args(["target", "list", "--installed"])
            .output()
        {
            if output.status.success() {
                return String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .any(|line| line.trim() == target);
            }
        }
    }
    let Ok(output) = Command::new(rustc)
        .args(["--print", "target-libdir", "--target", target])
        .output()
    else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    std::fs::read_dir(Path::new(&path)).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with("libstd-") && name.ends_with(".rlib")
        })
    })
}
