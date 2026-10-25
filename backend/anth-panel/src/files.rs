//! Reading one slice of a file the operator approved.
//!
//! A slice, not a stream: the cursor is the identity, so `offset` is all the
//! daemon keeps between calls and a client that loses an answer asks again.
//!
//! The root is the authority boundary. Every path is resolved — symlinks
//! included — and then checked to be inside the root, because a lexical prefix
//! check passes `/srv/../etc/shadow` and a symlink inside the root can point
//! anywhere. This is the whole of what makes `file read` safe to expose.

use std::path::{Component, Path, PathBuf};

use anth_wire::{ErrorKind, WireError};

/// Largest slice one call may return.
///
/// Matches the string limit a capability crosses the guest boundary with, so a
/// plugin and a CLI client page in the same steps. A caller asking for more is
/// refused, never given a short answer: silently shortening a read is the
/// v0.1.0 failure this rewrite exists to remove.
pub const MAX_SLICE: u32 = 64 * 1024;

/// What one read produced.
pub struct Slice {
    pub bytes: Vec<u8>,
    pub next: u64,
    pub eof: bool,
}

/// Reads `len` bytes at `offset` from `path`, inside `root`.
pub fn read(root: &Path, path: &str, offset: u64, len: u32) -> Result<Slice, WireError> {
    if len == 0 {
        return Err(WireError::new(
            ErrorKind::Invalid,
            "`len` must be greater than zero".to_string(),
        ));
    }
    if len > MAX_SLICE {
        return Err(WireError::new(
            ErrorKind::Invalid,
            format!("`len` of {len} exceeds the {MAX_SLICE} byte limit; read it in slices"),
        ));
    }

    let resolved = resolve(root, path)?;
    let mut file = std::fs::File::open(&resolved)
        .map_err(|e| WireError::new(ErrorKind::NotFound, format!("cannot open `{path}`: {e}")))?;

    let total = file
        .metadata()
        .map_err(|e| WireError::internal(format!("cannot stat `{path}`: {e}")))?
        .len();

    // Past the end is not an error: it is how a client discovers it has finished.
    if offset >= total {
        return Ok(Slice {
            bytes: Vec::new(),
            next: total,
            eof: true,
        });
    }

    use std::io::{Read, Seek, SeekFrom};
    file.seek(SeekFrom::Start(offset))
        .map_err(|e| WireError::internal(format!("cannot seek in `{path}`: {e}")))?;

    let mut buffer = vec![0u8; len as usize];
    let mut filled = 0;
    // `read` may return less than asked for without being at the end, so the
    // loop keeps going until the buffer is full or the file really ends.
    while filled < buffer.len() {
        match file.read(&mut buffer[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => {
                return Err(WireError::internal(format!("cannot read `{path}`: {e}")));
            }
        }
    }
    buffer.truncate(filled);

    let next = offset + filled as u64;
    Ok(Slice {
        bytes: buffer,
        next,
        eof: next >= total,
    })
}

/// Resolves `path` under `root`, refusing anything that escapes.
///
/// Returns the absolute path, so the caller opens exactly what was checked
/// rather than re-resolving and racing a symlink change.
pub fn resolve(root: &Path, path: &str) -> Result<PathBuf, WireError> {
    let root = root.canonicalize().map_err(|e| {
        WireError::new(
            ErrorKind::Forbidden,
            format!("the readable root `{}` is unusable: {e}", root.display()),
        )
    })?;

    let requested = Path::new(path);
    if requested.is_absolute() {
        return Err(forbidden(path, "is absolute; name it relative to the root"));
    }
    // Refused lexically as well as after resolution, so the message names the
    // `..` the caller typed instead of the directory it happened to reach.
    if requested
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
        return Err(forbidden(path, "contains `..`"));
    }

    let joined = root.join(requested);
    let real = joined
        .canonicalize()
        .map_err(|e| WireError::new(ErrorKind::NotFound, format!("cannot open `{path}`: {e}")))?;

    if !real.starts_with(&root) {
        return Err(forbidden(path, "resolves outside the readable root"));
    }
    Ok(real)
}

fn forbidden(path: &str, why: &str) -> WireError {
    WireError::new(ErrorKind::Forbidden, format!("`{path}` {why}"))
}

#[cfg(test)]
mod tests;
