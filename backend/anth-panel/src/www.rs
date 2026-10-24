//! The shell: `www/` served by the daemon.
//!
//! Two sources behind one lookup, chosen at compile time:
//!
//! - **release** embeds the files, so the binary runs from anywhere with no
//!   `www/` beside it;
//! - **debug** reads them from disk on every request, so rebuilding the
//!   frontend needs no Rust rebuild.
//!
//! The debug root is `CARGO_MANIFEST_DIR`, not rust-embed's default, which
//! resolves the folder relative to the *current working directory* and so
//! serves nothing when the daemon is started from anywhere but `backend/`.
//! A compile-time path cannot depend on where the process happens to be.

use std::borrow::Cow;
#[cfg(debug_assertions)]
use std::path::PathBuf;
use std::path::{Component, Path};

/// Files in `www/` that are never served.
///
/// `harness.html` is the development page that mounts a plugin's widget
/// directly. It is useful beside the dev server and has no business in a
/// deployment, so it is excluded from both sources rather than only from the
/// frontend's build output.
pub const EXCLUDED: &[&str] = &["harness.html"];

/// Whether `path` names a file the shell refuses to serve.
///
/// Separate from the lookup so the guarantee is one pure decision, checked the
/// same way whatever the source is. Release embeds its files at compile time,
/// so a test there cannot put `harness.html` in front of [`get`] to watch it be
/// refused — the only way to test that build's exclusion would be to ship the
/// file it exists to hide.
pub fn denied(path: &str) -> bool {
    EXCLUDED.contains(&path)
}

/// One file the shell answers with.
pub struct Asset {
    pub bytes: Cow<'static, [u8]>,
    pub content_type: &'static str,
}

#[cfg(not(debug_assertions))]
#[derive(rust_embed::RustEmbed)]
#[folder = "www"]
struct Shell;

/// Whether a router built right now could serve the shell.
///
/// A missing shell is a developer who has not run the frontend build, not a
/// visitor asking for a page that does not exist, and the two deserve different
/// answers: a `404` would send them hunting for a typo in a path that is fine.
///
/// `www/` is tracked in git, so a fresh checkout has the directory but not its
/// contents: "the directory exists" is not "a shell was built", and answering
/// the first as if it were the second reported a missing file as a missing
/// build. The index is the file that has to be there.
///
/// Whether a build could serve the shell, as the route handler sees it.
#[cfg(debug_assertions)]
pub fn available() -> bool {
    available_at(&root())
}

/// Whether `root` holds a shell, so the decision can be tested over a fixture
/// instead of over whatever this machine has staged.
#[cfg(debug_assertions)]
pub fn available_at(root: &Path) -> bool {
    root.join("index.html").is_file()
}

#[cfg(not(debug_assertions))]
pub fn available() -> bool {
    Shell::get("index.html").is_some()
}

/// Where a debug build reads the shell from.
///
/// `ANTH_WEB` overrides it so an installed tree can point at its own assets
/// without a rebuild. The compiled-in default is absolute, unlike the socket
/// and the bundle root: `www/` sits inside the crate, so a debug binary finds
/// its shell from any working directory. The test suite passes its own root to
/// [`lookup`] instead of reading this, because a test that depended on the
/// environment it runs in would pass here and fail elsewhere.
#[cfg(debug_assertions)]
pub fn root() -> PathBuf {
    match std::env::var("ANTH_WEB") {
        Ok(path) if !path.is_empty() => PathBuf::from(path),
        _ => PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/www")),
    }
}

/// The file `path` names, or `None`.
///
/// `path` is the request path with its leading slash already removed, so `""`
/// and `"/"` both mean the index.
pub fn get(path: &str) -> Option<Asset> {
    let path = normalise(path)?;
    if denied(&path) {
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

/// Reads one file from `root`, refusing anything that escapes it.
///
/// The shell is a build artifact this daemon produced, so traversal here is
/// defence in depth rather than the only guard: [`normalise`] rejects `..` and
/// absolute paths before this runs, and the canonical check then catches a
/// symlink pointing outside the tree.
#[cfg(debug_assertions)]
pub fn lookup(root: &Path, path: &str) -> Option<Asset> {
    let path = normalise(path)?;
    if denied(&path) {
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

/// Rejects a path that could leave the shell directory, or name a dotfile.
///
/// Returns the path with empty and `.` components dropped, so `a//b` and
/// `./a/b` both name `a/b`. A `..` or a root component is refused outright
/// rather than resolved: the shell has no directory above it to clamp to.
///
/// A name starting with `.` is refused too. The shell directory is tracked in
/// git, so a clean checkout holds `.gitkeep`; embedding it is harmless, serving
/// it is not. `.DS_Store` and a stray `.env` are the same shape of mistake.
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

/// The type to answer with, by extension.
///
/// A short explicit table rather than a `mime_guess` dependency: the shell is a
/// Vite build and ships these few kinds, and an unknown one is better answered
/// as opaque bytes than guessed at.
pub fn content_type_for(path: &Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("woff2") => "font/woff2",
        Some("png") => "image/png",
        _ => "application/octet-stream",
    }
}

/// Whether `path` names a file rather than a browser route.
///
/// The SPA fallback answers an unknown *route* with the index and an unknown
/// *file* with a `404`. Serving `index.html` for a missing `.js` hands the
/// browser HTML it will fail to parse, and the error then points at the module
/// rather than at the 404 that caused it.
pub fn looks_like_file(path: &str) -> bool {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.rsplit_once('.'))
        .is_some_and(|(stem, extension)| !stem.is_empty() && !extension.is_empty())
}
