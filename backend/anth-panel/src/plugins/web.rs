//! Declared plugin UI assets.
//!
//! Only files named by a `[[ui]]` slot in the grant are served. The path is
//! resolved inside `web/`, canonicalised, and must stay under the bundle; a
//! symlink that leaves it is refused. The content type comes from a host-owned
//! extension table, and every response is `nosniff` and `no-store`.

use std::path::Path;

use anth_wire::WireError;

use super::bundle::MAX_ASSET_BYTES;
use anth_extension::grants::ExtensionGrant as PluginGrant;

pub fn content_type(module: &str) -> &'static str {
    match module
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .as_deref()
    {
        Some("js") | Some("mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Reads one module the grant declares.
pub fn read_asset(root: &Path, grant: &PluginGrant, module: &str) -> Result<Vec<u8>, WireError> {
    let declared = grant.ui.iter().any(|slot| slot.module == module);
    if !declared {
        return Err(WireError::not_found(format!(
            "plugin `{}` does not declare the module `{module}`",
            grant.id
        )));
    }
    let dir = root.join(&grant.id);
    let base = dir
        .canonicalize()
        .map_err(|error| WireError::not_found(format!("reading plugin `{}`: {error}", grant.id)))?;
    let path = dir.join(module).canonicalize().map_err(|error| {
        WireError::not_found(format!("reading `{module}` from `{}`: {error}", grant.id))
    })?;
    if !path.starts_with(&base) || !path.is_file() {
        return Err(WireError::forbidden(format!(
            "`{module}` is not a regular file inside the bundle"
        )));
    }
    let metadata = std::fs::metadata(&path)
        .map_err(|error| WireError::internal(format!("reading {module}: {error}")))?;
    if metadata.len() > MAX_ASSET_BYTES {
        return Err(WireError::too_large(format!(
            "`{module}` is larger than {MAX_ASSET_BYTES} bytes"
        )));
    }
    std::fs::read(&path).map_err(|error| WireError::internal(format!("reading {module}: {error}")))
}
