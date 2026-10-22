//! Plugin web assets: `GET /api/plugins/<id>/web/<path>`.
//!
//! A UI anchor names a module under the bundle's `web/` directory, and the
//! browser has to be able to fetch it. That path is the one place a plugin can
//! put bytes into the panel's origin, so the confinement is the whole job:
//! the request is resolved inside `<bundle>/web/`, symlinks are refused rather
//! than followed, and the content type comes from a host-owned extension table
//! instead of the file.
//!
//! Serving a bundle asset is deliberately *not* an anchor call. An anchor is
//! forwarded to the guest, which means running wasm to answer a request for a
//! static file; the guest cannot read its own directory, and giving it that
//! ability to serve assets would hand it the filesystem for a job the host can
//! do. So this handler reads no manifest and runs no guest: the grant decides
//! whether the plugin exists and is enabled, and everything else is a file read
//! with a name check.

use std::io;
use std::path::{Component, Path, PathBuf};

use axum::body::Body;
use axum::extract::{Path as AxumPath, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};

use crate::api::ApiError;
use crate::app::AppState;
use crate::plugins::bundle;
use crate::plugins::manifest::validate_id;

/// Largest asset served. A UI module is JavaScript or a stylesheet plus small
/// images; anything larger belongs in the plugin's own service, not here.
pub const MAX_ASSET_BYTES: u64 = 4 * 1024 * 1024;

/// Extensions the host is willing to name.
///
/// The list is an allowlist, not a lookup that falls back to a guess. A plugin
/// ships code for the panel's origin, so the host decides what that code is
/// called; `nosniff` then stops the browser from second-guessing it. Anything
/// unlisted is `application/octet-stream` and downloads rather than runs.
fn content_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("js") | Some("mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("map") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("wasm") => "application/wasm",
        _ => "application/octet-stream",
    }
}

/// Turns a wildcard capture into a relative path, refusing anything that could
/// leave `web/`.
///
/// The capture arrives percent-decoded from axum, so `%2e%2e` has already become
/// `..` by the time it reaches this function; checking here covers both
/// spellings. Components are inspected individually rather than by searching the
/// string for `..`, because `a..b.js` is a legal name.
fn relative_asset(capture: &str) -> Option<PathBuf> {
    let mut path = PathBuf::new();
    for part in capture.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return None;
        }
        path.push(part);
    }
    // Belt and braces: a component that survived the loop must be a plain name.
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return None;
    }
    Some(path)
}

/// `GET /api/plugins/{id}/web/{*path}`.
pub async fn asset(
    State(state): State<AppState>,
    AxumPath((id, path)): AxumPath<(String, String)>,
) -> Result<Response, ApiError> {
    let config = state.config().await;

    if !config.plugins.enabled {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "plugins are disabled in the configuration",
        ));
    }
    validate_id(&id).map_err(|error| ApiError::bad_request(error.detail))?;

    // The grant, not the manifest: a bundle directory without a grant is a
    // plugin the admin has not approved, and its assets must not load either.
    let grant = config
        .plugins
        .grants
        .iter()
        .find(|grant| grant.id == id)
        .cloned()
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::NOT_FOUND,
                format!("no plugin `{id}` is installed"),
            )
        })?;
    if !grant.enabled {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            format!("plugin `{id}` is disabled"),
        ));
    }

    let relative = relative_asset(&path)
        .ok_or_else(|| ApiError::bad_request("`..` is not allowed in an asset path"))?;

    let root = bundle::plugin_dir(&config.plugins.root_path(), &id)
        .map_err(|error| ApiError::new(StatusCode::NOT_FOUND, error.detail))?;
    let web_root = root.join("web");

    // Blocking by nature; the size limit means the read cannot run long enough
    // to matter, but the stat and the canonicalize are syscalls all the same.
    let content_type = content_type(&relative);
    let read = tokio::task::spawn_blocking(move || read_asset(&web_root, &relative))
        .await
        .map_err(|error| ApiError::internal(format!("the asset worker failed: {error}")))?;
    let bytes = read.map_err(|detail| ApiError::new(StatusCode::NOT_FOUND, detail))?;

    let body = Body::from(bytes);
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type.to_string()),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
            // A cached module would outlive an uninstall, and the panel
            // reloads a plugin by reinstalling it under the same id.
            (header::CACHE_CONTROL, "no-store".to_string()),
        ],
        body,
    )
        .into_response())
}

/// Reads one asset, refusing a symlink and anything outside `web/`.
fn read_asset(web_root: &Path, relative: &Path) -> Result<Vec<u8>, String> {
    // `canonicalize` resolves the whole chain in one step, so a symlink at any
    // component — including one planted after install — is resolved before the
    // containment check rather than after it.
    let root = web_root.canonicalize().map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            format!("`{}` has no `web` directory", web_root.display())
        } else {
            format!("could not resolve `{}`: {error}", web_root.display())
        }
    })?;
    let target = root.join(relative);
    let resolved = target.canonicalize().map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => "no such asset".to_string(),
        _ => format!("could not resolve the asset: {error}"),
    })?;
    if !resolved.starts_with(&root) {
        return Err("the asset resolves outside the plugin's `web` directory".to_string());
    }

    let metadata =
        std::fs::metadata(&resolved).map_err(|error| format!("could not stat: {error}"))?;
    if !metadata.is_file() {
        return Err("no such asset".to_string());
    }
    if metadata.len() > MAX_ASSET_BYTES {
        return Err(format!("an asset is at most {MAX_ASSET_BYTES} bytes"));
    }
    // Opened through the resolved path, which contains no symlink component.
    std::fs::read(&resolved).map_err(|error| format!("could not read the asset: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_traversal_is_refused_in_every_spelling() {
        for capture in ["../plugin.toml", "a/../../b.js", "..", "a/./b.js", ""] {
            assert!(
                relative_asset(capture).is_none(),
                "`{capture}` must be refused"
            );
        }
    }

    #[test]
    fn a_double_dot_inside_a_name_is_not_a_traversal() {
        let path = relative_asset("a..b.js").expect("`a..b.js` is a plain name");
        assert_eq!(path, PathBuf::from("a..b.js"));
    }

    #[test]
    fn the_content_type_comes_from_the_extension_and_falls_back_to_octets() {
        assert_eq!(
            content_type(Path::new("x.js")),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(content_type(Path::new("x.CSS")), "text/css; charset=utf-8");
        assert_eq!(content_type(Path::new("x.exe")), "application/octet-stream");
        assert_eq!(content_type(Path::new("noext")), "application/octet-stream");
    }

    #[test]
    fn a_symlink_out_of_web_is_refused() {
        let dir = std::env::temp_dir().join(format!(
            "anth-webassets-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        let web = dir.join("web");
        std::fs::create_dir_all(&web).unwrap();
        std::fs::write(dir.join("plugin.toml"), "secret").unwrap();
        std::fs::write(web.join("ok.js"), "export {}").unwrap();
        std::os::unix::fs::symlink(dir.join("plugin.toml"), web.join("leak.js")).unwrap();

        assert!(read_asset(&web, Path::new("ok.js")).is_ok());
        let error = read_asset(&web, Path::new("leak.js")).unwrap_err();
        assert!(error.contains("outside"), "{error}");
        std::fs::remove_dir_all(&dir).ok();
    }
}
