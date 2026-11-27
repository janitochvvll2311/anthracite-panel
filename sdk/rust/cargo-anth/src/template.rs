//! Project scaffolding. The generated tree is standalone: it never references
//! this repository.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::config::{display_name, namespace, short_name, validate_id};

pub fn create(id: &str, lang: &str, dir: Option<&Path>) -> Result<PathBuf> {
    validate_id(id)?;
    if !matches!(lang, "rust" | "bun") {
        bail!("--lang must be `rust` or `bun`");
    }
    let target = match dir {
        Some(path) => path.to_path_buf(),
        None => PathBuf::from(short_name(id)),
    };
    if target.exists() {
        bail!("{} already exists", target.display());
    }
    fs::create_dir_all(&target).with_context(|| format!("creating {}", target.display()))?;
    let short = short_name(id);
    let ns = namespace(id);
    let name = display_name(id);
    let prefix = ns.split(':').next().unwrap_or(&ns);

    write(&target, "anth.toml", &anth_toml(id, lang))?;
    write(
        &target,
        "manifest.toml",
        &manifest(id, &ns, short, &name, lang),
    )?;
    write(&target, "Cargo.toml", &cargo_toml(short))?;
    write(&target, "wit/app.wit", &wit(&ns))?;
    write(
        &target,
        "src/lib.rs",
        &lib_rs(&prefix.replace('-', "_"), &short.replace('-', "_")),
    )?;
    write(
        &target,
        ".gitignore",
        "/target\n/dist\n/wasm\n*.key\nnode_modules/\n",
    )?;
    write(&target, "README.md", &readme(id, short, &name, lang))?;

    if lang == "bun" {
        write(&target, "package.json", &package_json(short))?;
        write(&target, "src/main.ts", &main_ts(&name))?;
        write(&target, "tsconfig.json", TSCONFIG)?;
    }

    println!("created {} ({})", target.display(), lang);
    println!("  cd {}", target.display());
    println!("  cargo anth app build");
    Ok(target)
}

fn write(root: &Path, relative: &str, contents: &str) -> Result<()> {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    fs::write(&path, contents).with_context(|| format!("writing {}", path.display()))
}

fn anth_toml(id: &str, lang: &str) -> String {
    format!(
        r#"# The Anthracite project file. `anth app build` reads it.
[app]
id = "{id}"
lang = "{lang}"

[build]
target = "wasm32-unknown-unknown"

[wit]
catalog = "0.8"
world = "app"
"#
    )
}

fn manifest(id: &str, ns: &str, short: &str, name: &str, lang: &str) -> String {
    let zero = "0".repeat(64);
    let slot = if lang == "bun" {
        format!(
            r#"
[[slot]]
id = "panel.view"
label = "{name}"
icon = "box"
order = 100
tag = "{short}"
module = "web/app.js"
sha256 = "{zero}"
"#
        )
    } else {
        String::new()
    };
    format!(
        r#"manifest_version = 2

[app]
id = "{id}"
namespace = "{ns}"
name = "{name}"
version = "0.1.0"
summary = "A new Anthracite app."
license = "Apache-2.0"
icon = "box"

[wasm]
path = "wasm/app.wasm"
sha256 = "{zero}"

[[export]]
interface = "{ns}/api@0.1.0"

[[route]]
method = "GET"
path = "/ping"
call = "{ns}/api@0.1.0#ping"
{slot}"#
    )
}

fn cargo_toml(short: &str) -> String {
    format!(
        r#"[package]
name = "{short}"
version = "0.1.0"
edition = "2021"
license = "Apache-2.0"

[lib]
crate-type = ["cdylib"]

[dependencies]
wit-bindgen = "0.63"

[profile.release]
opt-level = "s"
lto = true
codegen-units = 1
panic = "abort"
strip = true
"#
    )
}

fn wit(ns: &str) -> String {
    format!(
        r#"package {ns}@0.1.0;

interface api {{
    ping: func() -> string;
}}

world app {{
    export api;
}}
"#
    )
}

fn lib_rs(prefix: &str, short: &str) -> String {
    format!(
        r#"//! The wasm component. Replace `ping` with the app's exports.

wit_bindgen::generate!({{
    path: "wit",
    world: "app",
}});

use exports::{prefix}::{short}::api::Guest;

struct App;

impl Guest for App {{
    fn ping() -> String {{
        "pong".to_string()
    }}
}}

export!(App);
"#
    )
}

fn package_json(short: &str) -> String {
    format!(
        r#"{{
  "name": "{short}",
  "private": true,
  "type": "module",
  "scripts": {{
    "build": "bun build src/main.ts --outfile web/app.js --format esm --target browser"
  }},
  "devDependencies": {{
    "typescript": "5.9.3"
  }}
}}
"#
    )
}

fn main_ts(name: &str) -> String {
    format!(
        r#"// The panel module. It exports the `mount` entry point the panel calls.
export function mount(
  root: HTMLElement,
  context: {{ app: {{ id: string; version: string }} }},
): void {{
  root.innerHTML = "";
  const heading = document.createElement("h2");
  heading.textContent = "{name}";
  const line = document.createElement("p");
  line.textContent = `${{context.app.id}} · v${{context.app.version}}`;
  root.append(heading, line);
}}
"#
    )
}

fn readme(id: &str, short: &str, name: &str, lang: &str) -> String {
    let ui = if lang == "bun" {
        "\nThe UI lives in `src/main.ts`; `cargo anth app build` runs `bun build` into\n`web/app.js` and the manifest pins its hash.\n"
    } else {
        ""
    };
    format!(
        r#"# {name}

`{id}` — an Anthracite app.

```sh
anth-sign keygen --out dev.key
cargo anth app build
cargo anth app check
cargo anth app package --key dev.key
cargo anth app verify --public-key <public-key>
```

`cargo anth app build` compiles `{short}` for wasm32-unknown-unknown, wraps it with
`wasm-tools`, and updates the manifest hashes.{ui}
`anth-sign` and `anth-conformance` ship in the GitHub Release `bin/`; put that
directory on `PATH`. The generated tree is standalone; it does not depend on
the Anthracite repository.
"#
    )
}

const TSCONFIG: &str = r#"{
  "compilerOptions": {
    "target": "ES2022",
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "strict": true,
    "skipLibCheck": true,
    "noEmit": true,
    "lib": ["ES2022", "DOM"]
  },
  "include": ["src/**/*.ts"]
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rust_project_is_standalone() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("notes");
        create("org.example.notes", "rust", Some(&root)).expect("create");
        for file in [
            "anth.toml",
            "manifest.toml",
            "Cargo.toml",
            "wit/app.wit",
            "src/lib.rs",
            "README.md",
        ] {
            assert!(root.join(file).is_file(), "{file} is missing");
        }
        assert!(!root.join("package.json").exists());
        let wit = std::fs::read_to_string(root.join("wit/app.wit")).expect("wit");
        assert!(wit.contains("package example:notes@0.1.0;"));
        let manifest = std::fs::read_to_string(root.join("manifest.toml")).expect("manifest");
        assert!(manifest.contains("id = \"org.example.notes\""));
        assert!(manifest.contains("example:notes/api@0.1.0"));
    }

    #[test]
    fn a_bun_project_adds_a_panel_module() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("dashboard");
        create("org.example.dashboard", "bun", Some(&root)).expect("create");
        for file in ["package.json", "src/main.ts", "tsconfig.json"] {
            assert!(root.join(file).is_file(), "{file} is missing");
        }
        let manifest = std::fs::read_to_string(root.join("manifest.toml")).expect("manifest");
        assert!(manifest.contains("module = \"web/app.js\""));
    }

    #[test]
    fn an_existing_directory_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("notes");
        create("org.example.notes", "rust", Some(&root)).expect("create");
        assert!(create("org.example.notes", "rust", Some(&root)).is_err());
    }
}
