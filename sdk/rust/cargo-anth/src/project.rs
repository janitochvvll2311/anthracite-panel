//! Build, package, sign, verify and check one app project.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};

use crate::config::{self, Project};

pub fn build(project: &Project) -> Result<PathBuf> {
    let root = &project.root;
    let cargo = tool("CARGO", "cargo")?;
    let target = &project.config.build.target;

    run(
        Command::new(&cargo)
            .current_dir(root)
            .arg("build")
            .arg("--manifest-path")
            .arg(root.join("Cargo.toml"))
            .arg("--release")
            .arg("--target")
            .arg(target),
        "cargo build",
    )?;

    let metadata = Command::new(&cargo)
        .current_dir(root)
        .arg("metadata")
        .arg("--format-version")
        .arg("1")
        .arg("--no-deps")
        .arg("--manifest-path")
        .arg(root.join("Cargo.toml"))
        .output()
        .context("running cargo metadata")?;
    if !metadata.status.success() {
        bail!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&metadata.stderr).trim()
        );
    }
    let metadata: serde_json::Value =
        serde_json::from_slice(&metadata.stdout).context("parsing cargo metadata")?;
    let target_dir = metadata["target_directory"]
        .as_str()
        .context("cargo metadata has no target_directory")?;
    let package_name = metadata["packages"]
        .as_array()
        .and_then(|packages| {
            packages
                .iter()
                .find(|package| {
                    package["manifest_path"]
                        .as_str()
                        .is_some_and(|path| Path::new(path) == root.join("Cargo.toml"))
                })
                .or_else(|| packages.first())
        })
        .and_then(|package| package["name"].as_str())
        .context("cargo metadata has no package name")?;
    let artifact = project
        .config
        .build
        .artifact
        .clone()
        .unwrap_or_else(|| package_name.replace('-', "_"));
    let module = Path::new(target_dir)
        .join(target)
        .join("release")
        .join(format!("{artifact}.wasm"));
    if !module.is_file() {
        bail!(
            "cargo did not produce {}; check build.target and the crate-type",
            module.display()
        );
    }

    let wasm_tools = tool("WASM_TOOLS", "wasm-tools")?;
    let temp = tempfile::tempdir().context("creating a temporary directory")?;
    let embedded = temp.path().join("embedded.wasm");
    run(
        Command::new(&wasm_tools)
            .current_dir(root)
            .arg("component")
            .arg("embed")
            .arg("--world")
            .arg(&project.config.wit.world)
            .arg(root.join("wit"))
            .arg(&module)
            .arg("-o")
            .arg(&embedded),
        "wasm-tools component embed",
    )?;
    let output = root.join("wasm/app.wasm");
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    run(
        Command::new(&wasm_tools)
            .arg("component")
            .arg("new")
            .arg(&embedded)
            .arg("-o")
            .arg(&output),
        "wasm-tools component new",
    )?;

    if project.config.app.lang == "bun" {
        let bun = tool("BUN", "bun")?;
        run(
            Command::new(&bun)
                .current_dir(root)
                .arg("build")
                .arg("src/main.ts")
                .arg("--outfile")
                .arg("web/app.js")
                .arg("--format")
                .arg("esm")
                .arg("--target")
                .arg("browser"),
            "bun build",
        )?;
    }

    update_hashes(project, &output)?;
    println!("built {}", output.display());
    Ok(output)
}

/// Rewrites `[wasm].sha256` and every `web/*` slot hash from the files on disk.
fn update_hashes(project: &Project, wasm: &Path) -> Result<()> {
    let path = project.manifest();
    let text = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .with_context(|| format!("parsing {}", path.display()))?;

    doc["wasm"]["sha256"] = toml_edit::value(hash(wasm)?);

    if let Some(slots) = doc["slot"].as_array_of_tables_mut() {
        for slot in slots.iter_mut() {
            let Some(module) = slot.get("module").and_then(|value| value.as_str()) else {
                continue;
            };
            if !module.starts_with("web/") {
                continue;
            }
            let file = project.root.join(module);
            if file.is_file() {
                slot["sha256"] = toml_edit::value(hash(&file)?);
            }
        }
    }

    fs::write(&path, doc.to_string()).with_context(|| format!("writing {}", path.display()))
}

pub fn package(project: &Project, key: Option<&Path>) -> Result<PathBuf> {
    let stage = stage(project)?;
    if let Some(key) = key {
        sign_dir(&stage, key)?;
    }
    fs::create_dir_all(project.dist())
        .with_context(|| format!("creating {}", project.dist().display()))?;
    let bundle = project.bundle();
    zip_dir(&stage, &bundle)?;
    println!("wrote {}", bundle.display());
    Ok(bundle)
}

/// Builds the app and lays out the unpacked bundle under `target/anth/<id>`.
fn stage(project: &Project) -> Result<PathBuf> {
    build(project)?;
    let stage = project
        .root
        .join("target/anth")
        .join(&project.config.app.id);
    if stage.exists() {
        fs::remove_dir_all(&stage).with_context(|| format!("removing {}", stage.display()))?;
    }
    fs::create_dir_all(&stage).with_context(|| format!("creating {}", stage.display()))?;

    copy(&project.manifest(), &stage.join("manifest.toml"))?;
    let wit = project.root.join("wit");
    let wit_out = stage.join("wit");
    fs::create_dir_all(&wit_out).context("creating the staged wit directory")?;
    for entry in fs::read_dir(&wit).with_context(|| format!("reading {}", wit.display()))? {
        let entry = entry?;
        if entry.file_type()?.is_file() && entry.path().extension().is_some_and(|ext| ext == "wit")
        {
            copy(&entry.path(), &wit_out.join(entry.file_name()))?;
        }
    }

    let manifest = manifest_value(project)?;
    let wasm = manifest
        .get("wasm")
        .and_then(|wasm| wasm.get("path"))
        .and_then(toml::Value::as_str)
        .unwrap_or("wasm/app.wasm");
    copy(&project.root.join(wasm), &stage.join(wasm))?;
    if project.root.join("web").is_dir() {
        copy_tree(&project.root.join("web"), &stage.join("web"))?;
    }
    Ok(stage)
}

pub fn sign(project: &Project, key: &Path) -> Result<PathBuf> {
    let bundle = project.bundle();
    if !bundle.is_file() {
        bail!(
            "{} does not exist; run `anth app package` first",
            bundle.display()
        );
    }
    let temp = tempfile::tempdir().context("creating a temporary directory")?;
    unzip(&bundle, temp.path())?;
    sign_dir(temp.path(), key)?;
    zip_dir(temp.path(), &bundle)?;
    println!("signed {}", bundle.display());
    Ok(bundle)
}

pub fn verify(project: &Project, public_key: Option<&str>) -> Result<()> {
    let bundle = project.bundle();
    if !bundle.is_file() {
        bail!(
            "{} does not exist; run `anth app package` first",
            bundle.display()
        );
    }
    let conformance = tool("ANTH_CONFORMANCE", "anth-conformance")?;
    let mut command = Command::new(conformance);
    command.arg(&bundle);
    if let Some(key) = public_key {
        command.arg("--require-signature").arg("--key").arg(key);
    }
    run(&mut command, "anth-conformance")?;
    println!("verified {}", bundle.display());
    Ok(())
}

pub fn check(project: &Project) -> Result<()> {
    let stage = stage(project)?;
    let temp = tempfile::tempdir().context("creating a temporary directory")?;
    let bundle = temp.path().join("check.anth");
    zip_dir(&stage, &bundle)?;
    let conformance = tool("ANTH_CONFORMANCE", "anth-conformance")?;
    run(Command::new(conformance).arg(&bundle), "anth-conformance")?;
    println!("checked {}", bundle.display());
    Ok(())
}

fn sign_dir(dir: &Path, key: &Path) -> Result<()> {
    let signer = tool("ANTH_SIGN", "anth-sign")?;
    run(
        Command::new(signer).arg(dir).arg("--key").arg(key),
        "anth-sign",
    )
}

fn manifest_value(project: &Project) -> Result<toml::Value> {
    let text = fs::read_to_string(project.manifest())
        .with_context(|| format!("reading {}", project.manifest().display()))?;
    toml::from_str(&text).context("parsing manifest.toml")
}

fn hash(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(hex(&Sha256::digest(bytes)))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn copy(from: &Path, to: &Path) -> Result<()> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    fs::copy(from, to)
        .with_context(|| format!("copying {} to {}", from.display(), to.display()))?;
    Ok(())
}

fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to).with_context(|| format!("creating {}", to.display()))?;
    for entry in fs::read_dir(from).with_context(|| format!("reading {}", from.display()))? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            copy(&entry.path(), &target)?;
        }
    }
    Ok(())
}

fn zip_dir(from: &Path, to: &Path) -> Result<()> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let file = fs::File::create(to).with_context(|| format!("creating {}", to.display()))?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    add_zip(&mut zip, from, from, options)?;
    zip.finish().context("finishing the bundle zip")?;
    Ok(())
}

fn add_zip(
    zip: &mut zip::ZipWriter<fs::File>,
    root: &Path,
    dir: &Path,
    options: zip::write::SimpleFileOptions,
) -> Result<()> {
    let mut entries = fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .with_context(|| format!("{} is outside {}", path.display(), root.display()))?;
        let name = relative.to_string_lossy().replace('\\', "/");
        if entry.file_type()?.is_dir() {
            add_zip(zip, root, &path, options)?;
        } else {
            zip.start_file(name, options)
                .with_context(|| format!("adding {}", path.display()))?;
            let mut file =
                fs::File::open(&path).with_context(|| format!("opening {}", path.display()))?;
            std::io::copy(&mut file, zip).with_context(|| format!("writing {}", path.display()))?;
        }
    }
    Ok(())
}

fn unzip(from: &Path, to: &Path) -> Result<()> {
    let file = fs::File::open(from).with_context(|| format!("opening {}", from.display()))?;
    let mut archive = zip::ZipArchive::new(file).context("reading the bundle zip")?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let Some(name) = entry.enclosed_name() else {
            bail!("bundle contains an unsafe path: {}", entry.name());
        };
        let target = to.join(name);
        if entry.is_dir() {
            fs::create_dir_all(&target)
                .with_context(|| format!("creating {}", target.display()))?;
        } else {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("creating {}", parent.display()))?;
            }
            let mut output = fs::File::create(&target)
                .with_context(|| format!("creating {}", target.display()))?;
            let mut buffer = Vec::new();
            entry.read_to_end(&mut buffer)?;
            output.write_all(&buffer)?;
        }
    }
    Ok(())
}

fn tool(env: &str, name: &str) -> Result<String> {
    if let Ok(value) = std::env::var(env) {
        if !value.is_empty() {
            return Ok(value);
        }
    }
    if let Some(path) = find_in_path(name) {
        return Ok(path);
    }
    for fallback in [
        format!("/tools/cargo/bin/{name}"),
        format!("/tools/bun/bin/{name}"),
    ] {
        if Path::new(&fallback).is_file() {
            return Ok(fallback);
        }
    }
    bail!("`{name}` is not on PATH; set {env} or install the toolchain")
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

fn run(command: &mut Command, label: &str) -> Result<()> {
    let status = command
        .status()
        .with_context(|| format!("running {label}"))?;
    if !status.success() {
        bail!("{label} exited with {status}");
    }
    Ok(())
}

/// The key from `[sign].key`, if set.
pub fn configured_key(project: &Project) -> Result<Option<PathBuf>> {
    let Some(sign) = &project.config.sign else {
        return Ok(None);
    };
    let Some(value) = sign.key.as_deref() else {
        return Ok(None);
    };
    Ok(Some(config::key_path(project, value)?))
}
