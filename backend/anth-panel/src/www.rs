//! The embedded web shell.
//!
//! Release builds embed `www/`; debug builds read it from `CARGO_MANIFEST_DIR`
//! so a frontend rebuild needs no Rust rebuild.

use std::borrow::Cow;
#[cfg(debug_assertions)]
use std::path::PathBuf;
use std::path::{Component, Path};

/// Files that are never served from the shell.
pub const EXCLUDED: &[&str] = &["harness.html"];

pub struct Asset {
    pub bytes: Cow<'static, [u8]>,
    pub content_type: &'static str,
}

#[cfg(not(debug_assertions))]
#[derive(rust_embed::RustEmbed)]
#[folder = "www"]
struct Shell;

/// Whether a build could serve the shell.
pub fn available() -> bool {
    get("index.html").is_some()
}

#[cfg(debug_assertions)]
pub fn root() -> PathBuf {
    match std::env::var("ANTH_WEB") {
        Ok(path) if !path.is_empty() => PathBuf::from(path),
        _ => PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/www")),
    }
}

/// The file `path` names, or `None`.
pub fn get(path: &str) -> Option<Asset> {
    let path = normalise(path)?;
    if EXCLUDED.contains(&path.as_str()) {
        return None;
    }
    if path.is_empty() {
        return get("index.html");
    }

    #[cfg(debug_assertions)]
    {
        lookup(&root(), &path)
    }
    #[cfg(not(debug_assertions))]
    {
        Shell::get(&path).map(|file| Asset {
            bytes: file.data,
            content_type: content_type_for(Path::new(&path)),
        })
    }
}

#[cfg(debug_assertions)]
pub fn lookup(root: &Path, path: &str) -> Option<Asset> {
    let path = normalise(path)?;
    if EXCLUDED.contains(&path.as_str()) {
        return None;
    }
    let resolved = root.join(&path).canonicalize().ok()?;
    let base = root.canonicalize().ok()?;
    if !resolved.starts_with(&base) || !resolved.is_file() {
        return None;
    }
    let bytes = std::fs::read(&resolved).ok()?;
    Some(Asset {
        bytes: Cow::Owned(bytes),
        content_type: content_type_for(Path::new(&path)),
    })
}

/// Rejects traversal and dotfiles, and drops harmless `.` components.
pub fn normalise(path: &str) -> Option<String> {
    let mut parts = Vec::new();
    for component in Path::new(path).components() {
        match component {
            Component::Normal(part) => {
                let part = part.to_str()?;
                if part.starts_with('.') {
                    return None;
                }
                parts.push(part);
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(parts.join("/"))
}

/// Whether `path` names a file rather than an SPA route.
pub fn looks_like_file(path: &str) -> bool {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.rsplit_once('.'))
        .is_some_and(|(stem, extension)| !stem.is_empty() && !extension.is_empty())
}

pub fn content_type_for(path: &Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("woff2") => "font/woff2",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalise_rejects_traversal_and_dotfiles() {
        assert_eq!(normalise("a//b").as_deref(), Some("a/b"));
        assert_eq!(normalise("./a/b").as_deref(), Some("a/b"));
        assert_eq!(normalise("../a"), None);
        assert_eq!(normalise("/a"), None);
        assert_eq!(normalise(".gitkeep"), None);
    }

    #[test]
    fn harness_is_never_served() {
        assert!(EXCLUDED.contains(&"harness.html"));
        assert_eq!(normalise("harness.html").as_deref(), Some("harness.html"));
    }

    #[test]
    fn content_types_are_stable() {
        assert_eq!(
            content_type_for(Path::new("index.html")),
            "text/html; charset=utf-8"
        );
        assert_eq!(
            content_type_for(Path::new("app.js")),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(
            content_type_for(Path::new("app.css")),
            "text/css; charset=utf-8"
        );
        assert_eq!(content_type_for(Path::new("font.woff2")), "font/woff2");
    }
}
