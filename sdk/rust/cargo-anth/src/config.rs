//! `anth.toml`: the project file shared by the Rust and Bun workflows.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Deserialize;

pub const FILE: &str = "anth.toml";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub app: App,
    #[serde(default)]
    pub build: Build,
    #[serde(default)]
    pub wit: Wit,
    #[serde(default)]
    pub sign: Option<Sign>,
    /// Reserved for the dev server; parsed so the file is forward-compatible.
    #[allow(dead_code)]
    #[serde(default)]
    pub panel: Option<Panel>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct App {
    pub id: String,
    /// `rust` (a wasm app) or `bun` (a wasm app with a Bun UI module).
    #[serde(default = "default_lang")]
    pub lang: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Build {
    #[serde(default = "default_target")]
    pub target: String,
    /// Optional artifact name; defaults to the Cargo package name.
    #[serde(default)]
    pub artifact: Option<String>,
}

impl Default for Build {
    fn default() -> Self {
        Self {
            target: default_target(),
            artifact: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Wit {
    /// The catalog version the project was generated for.
    #[allow(dead_code)]
    #[serde(default = "default_catalog")]
    pub catalog: String,
    #[serde(default = "default_world")]
    pub world: String,
}

impl Default for Wit {
    fn default() -> Self {
        Self {
            catalog: default_catalog(),
            world: default_world(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sign {
    /// A path, or `env:NAME` for a variable that holds the path.
    pub key: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Panel {
    #[allow(dead_code)]
    #[serde(default = "default_port")]
    pub dev_port: u16,
}

fn default_lang() -> String {
    "rust".to_string()
}

fn default_target() -> String {
    "wasm32-unknown-unknown".to_string()
}

fn default_catalog() -> String {
    "0.8".to_string()
}

fn default_world() -> String {
    "app".to_string()
}

fn default_port() -> u16 {
    5173
}

pub struct Project {
    pub root: PathBuf,
    pub config: Config,
}

impl Project {
    /// Finds the project from an explicit directory or the current directory,
    /// walking up to the first `anth.toml`.
    pub fn load(project: Option<&Path>) -> Result<Self> {
        let start = match project {
            Some(path) => path.to_path_buf(),
            None => std::env::current_dir().context("reading the current directory")?,
        };
        let start = if start.is_file() {
            start
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| start.clone())
        } else {
            start
        };
        let mut dir = start.as_path();
        loop {
            let candidate = dir.join(FILE);
            if candidate.is_file() {
                let text = fs::read_to_string(&candidate)
                    .with_context(|| format!("reading {}", candidate.display()))?;
                let config: Config = toml::from_str(&text)
                    .with_context(|| format!("parsing {}", candidate.display()))?;
                validate(&config)?;
                return Ok(Self {
                    root: dir.to_path_buf(),
                    config,
                });
            }
            match dir.parent() {
                Some(parent) => dir = parent,
                None => bail!(
                    "no {FILE} above {}; run `anth app new` or pass --project",
                    start.display()
                ),
            }
        }
    }

    pub fn manifest(&self) -> PathBuf {
        self.root.join("manifest.toml")
    }

    pub fn dist(&self) -> PathBuf {
        self.root.join("dist")
    }

    pub fn bundle(&self) -> PathBuf {
        let id = &self.config.app.id;
        let version = manifest_version(&self.manifest()).unwrap_or_else(|_| "0.0.0".to_string());
        self.dist().join(format!("{id}-{version}.anth"))
    }
}

fn manifest_version(path: &Path) -> Result<String> {
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let value: toml::Value =
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    value
        .get("app")
        .and_then(|app| app.get("version"))
        .and_then(toml::Value::as_str)
        .map(str::to_string)
        .context("the manifest has no app.version")
}

fn validate(config: &Config) -> Result<()> {
    validate_id(&config.app.id)?;
    match config.app.lang.as_str() {
        "rust" | "bun" => {}
        other => bail!("app.lang must be `rust` or `bun`, not `{other}`"),
    }
    if config.build.target.is_empty() {
        bail!("build.target cannot be empty");
    }
    Ok(())
}

/// A reverse-DNS id: at least two labels, ASCII alphanumerics and `-`, no empty
/// label and no leading or trailing `-`.
pub fn validate_id(id: &str) -> Result<()> {
    let labels: Vec<&str> = id.split('.').collect();
    if labels.len() < 2 {
        bail!("`{id}` is not a reverse-DNS id (for example org.example.notes)");
    }
    for label in &labels {
        if label.is_empty()
            || label.starts_with('-')
            || label.ends_with('-')
            || !label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            bail!("`{id}` has an invalid label `{label}`");
        }
    }
    Ok(())
}

/// The WIT namespace of an id: the last two labels, `example:notes`.
pub fn namespace(id: &str) -> String {
    let labels: Vec<&str> = id.split('.').collect();
    let len = labels.len();
    format!("{}:{}", labels[len - 2], labels[len - 1])
}

/// The last label, used for the directory and the default app name.
pub fn short_name(id: &str) -> &str {
    id.rsplit('.').next().unwrap_or(id)
}

/// A display name: `notes-app` -> `Notes App`.
pub fn display_name(id: &str) -> String {
    short_name(id)
        .split('-')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Resolves `env:NAME` or a path relative to the project root.
pub fn key_path(project: &Project, value: &str) -> Result<PathBuf> {
    if let Some(name) = value.strip_prefix("env:") {
        let path = std::env::var(name)
            .with_context(|| format!("environment variable `{name}` is not set"))?;
        return Ok(PathBuf::from(path));
    }
    let path = Path::new(value);
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(project.root.join(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_reverse_dns() {
        validate_id("org.example.notes").expect("valid");
        validate_id("org.example.my-app").expect("valid");
        assert!(validate_id("notes").is_err());
        assert!(validate_id("org..notes").is_err());
        assert!(validate_id("org.example.-notes").is_err());
        assert!(validate_id("org.example.no_tes").is_err());
    }

    #[test]
    fn namespace_uses_the_last_two_labels() {
        assert_eq!(namespace("org.example.notes"), "example:notes");
        assert_eq!(namespace("com.acme.widget"), "acme:widget");
        assert_eq!(short_name("org.example.notes"), "notes");
        assert_eq!(display_name("org.example.notes-app"), "Notes App");
    }

    #[test]
    fn a_project_is_found_from_a_subdirectory() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join(FILE),
            "[app]\nid = \"org.example.notes\"\nlang = \"rust\"\n",
        )
        .expect("write");
        std::fs::create_dir(dir.path().join("src")).expect("mkdir");
        let project = Project::load(Some(&dir.path().join("src"))).expect("load");
        assert_eq!(project.config.app.id, "org.example.notes");
        assert_eq!(project.root, dir.path());
    }
}
