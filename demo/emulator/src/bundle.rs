//! Builds the demo's app bundles: one zip per app, one shared emulator binary.
//!
//! App manifests, WIT and frontend modules come from `apps/<name>`; the binary
//! is the deterministic emulator service. The binary is hashed into each
//! manifest and the bundle carries exactly one WIT package, which is what the
//! engine's validator requires.

use std::error::Error;
use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;

/// One bundle template and the app id its manifest declares.
pub const BUNDLES: &[(&str, &str)] = &[
    ("fs", "org.anthracite.fs"),
    ("terminal", "org.anthracite.terminal"),
    ("proc", "org.anthracite.proc"),
    ("net", "org.anthracite.net"),
    ("storage", "org.anthracite.storage"),
    ("accounts", "org.anthracite.accounts"),
];

#[derive(Debug)]
pub struct Built {
    pub app: String,
    pub path: PathBuf,
}

/// Renders every bundle under `out` from `binary`.
pub fn build(binary: &Path, out: &Path) -> Result<Vec<Built>, Box<dyn Error>> {
    let bytes = fs::read(binary)?;
    let hash: String = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps");
    fs::create_dir_all(out)?;
    let mut built = Vec::with_capacity(BUNDLES.len());
    for (name, app) in BUNDLES {
        let dir = root.join(name);
        let template = fs::read_to_string(dir.join("manifest.toml.in"))?;
        let module_path = dir.join("frontend").join("dist").join(format!("{name}.js"));
        let module = fs::read(&module_path).map_err(|error| {
            format!(
                "{} is missing: {error}; run apps/build-frontends.sh",
                module_path.display()
            )
        })?;
        let module_name = module_path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| format!("{} is not UTF-8", module_path.display()))?
            .to_string();
        let module_hash: String = Sha256::digest(&module)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let (app_name, summary, service_class, service_argv) = match *name {
            "fs" => (
                "Files (emulator)",
                "Deterministic filesystem fixture for the demo; replaced by the real fs app.",
                "socket",
                r#"["--socket", "{socket}", "--state", "{data}/state.json"]"#,
            ),
            "terminal" => (
                "Terminal (emulator)",
                "Scripted terminal fixture for the demo; replaced by the real terminal app.",
                "socket",
                r#"["--socket", "{socket}"]"#,
            ),
            "proc" => (
                "Processes (emulator)",
                "Deterministic process table fixture for the demo; replaced by the real proc app.",
                "socket",
                r#"["--socket", "{socket}"]"#,
            ),
            _ => ("", "", "", ""),
        };
        let manifest = template
            .replace("{{sha256}}", &hash)
            .replace("{{module_sha256}}", &module_hash)
            .replace("{{app_name}}", app_name)
            .replace("{{app_summary}}", summary)
            .replace("{{service_path}}", "bin/host-emulator")
            .replace("{{service_class}}", service_class)
            .replace("{{service_argv}}", service_argv);
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default();
        writer.start_file("manifest.toml", options)?;
        writer.write_all(manifest.as_bytes())?;
        let mut wit: Vec<PathBuf> = fs::read_dir(dir.join("wit"))?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<_, _>>()?;
        wit.sort();
        for path in wit {
            let file = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| format!("{} is not UTF-8", path.display()))?;
            writer.start_file(format!("wit/{file}"), options)?;
            writer.write_all(&fs::read(&path)?)?;
        }
        writer.start_file(format!("web/{module_name}"), options)?;
        writer.write_all(&module)?;
        writer.start_file("bin/host-emulator", options)?;
        writer.write_all(&bytes)?;
        let path = out.join(format!("{name}.anth"));
        fs::write(&path, writer.finish()?.into_inner())?;
        built.push(Built {
            app: (*app).to_string(),
            path,
        });
    }
    Ok(built)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anth_contract::Manifest;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("anthracite-panel").join(format!(
            "anth-emulator-bundle-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    #[test]
    fn every_bundle_validates_and_carries_its_package() {
        let dir = scratch("bundles");
        let binary = dir.join("host-emulator");
        fs::write(&binary, b"\x7fELF a deterministic fixture").expect("binary");
        let out = dir.join("dist");
        let built = build(&binary, &out).expect("build");
        assert_eq!(built.len(), BUNDLES.len());
        for bundle in &built {
            let bytes = fs::read(&bundle.path).expect("read bundle");
            let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
            let mut names: Vec<String> = (0..archive.len())
                .map(|index| archive.by_index(index).expect("entry").name().to_string())
                .collect();
            names.sort();
            assert!(names.contains(&"manifest.toml".to_string()), "{names:?}");
            assert!(
                names.contains(&"bin/host-emulator".to_string()),
                "{names:?}"
            );
            let mut manifest = String::new();
            std::io::Read::read_to_string(
                &mut archive.by_name("manifest.toml").expect("manifest"),
                &mut manifest,
            )
            .expect("read manifest");
            let manifest = Manifest::parse(&manifest).expect("parse");
            manifest.validate().expect("validate");
            assert_eq!(manifest.app.id, bundle.app);
            assert!(manifest.service.is_some());
            let slot = manifest.slot.first().expect("a panel.view slot");
            assert_eq!(slot.id, "panel.view");
            let mut module = Vec::new();
            std::io::Read::read_to_end(
                &mut archive.by_name(&slot.module).expect("module"),
                &mut module,
            )
            .expect("read module");
            let hash: String = Sha256::digest(&module)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            assert_eq!(hash, slot.sha256);
            let wit_name = names
                .iter()
                .find(|name| name.starts_with("wit/"))
                .expect("wit");
            let mut wit = Vec::new();
            std::io::Read::read_to_end(&mut archive.by_name(wit_name).expect("wit"), &mut wit)
                .expect("read wit");
            let text = String::from_utf8(wit).expect("utf8");
            assert!(
                text.starts_with(&format!("package {}@", manifest.app.namespace)),
                "{text}"
            );
            let wit_dir = dir.join("scratch").join(&manifest.app.id).join("wit");
            fs::create_dir_all(&wit_dir).expect("wit dir");
            fs::write(wit_dir.join("app.wit"), &text).expect("write wit");
            let group = wit_parser::UnresolvedPackageGroup::parse_dir(&wit_dir).expect("valid WIT");
            assert!(group.nested.is_empty());
            assert_eq!(
                format!("{}:{}", group.main.name.namespace, group.main.name.name),
                manifest.app.namespace
            );
        }
        let _ = fs::remove_dir_all(dir);
    }
}
