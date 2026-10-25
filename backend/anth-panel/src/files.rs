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

/// The longest a request may wait for bytes that have not arrived.
///
/// A caller asks for a wait; the daemon decides the ceiling. This is the whole
/// reason a follow request cannot hang: whatever the caller sends, the answer
/// comes back within this, so a client's own timeout only ever means "the
/// daemon is gone", never "the file was quiet".
pub const MAX_WAIT_MS: u32 = 30_000;

/// How often a waiting read looks again.
///
/// A follow is a request per interval, not a stream, so this only has to be
/// small enough that a log line appears promptly. Polling is what a file
/// offers: there is no portable "tell me when this grows" without inotify,
/// which would be one more thing the daemon has to own.
const POLL_MS: u64 = 50;

/// What one read produced.
pub struct Slice {
    pub bytes: Vec<u8>,
    pub next: u64,
    pub eof: bool,
    /// Whether the daemon waited for bytes that never arrived.
    ///
    /// An empty slice and `waited` is the honest answer to a follower: the file
    /// did not grow. Without this flag that answer is indistinguishable from an
    /// empty file, which is the silent truncation this rewrite removes.
    pub waited: bool,
}

/// Reads `len` bytes at `offset` from `path`, inside `root`.
///
/// `wait_ms` is how long to wait for those bytes to appear. Zero reads what is
/// there now; anything else is a follower, and it is capped by [`MAX_WAIT_MS`]
/// so the daemon always answers.
pub fn read(
    root: &Path,
    path: &str,
    offset: u64,
    len: u32,
    wait_ms: u32,
) -> Result<Slice, WireError> {
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

    // The type is checked with `metadata` on the *path*, before opening it: on a
    // FIFO, `open` blocks until a writer appears, so opening first would hang
    // the daemon on a path it is about to refuse. A cursor is also only
    // meaningful on something that can be seeked, and a device can block a read
    // forever, so both are refused.
    let meta = std::fs::metadata(&resolved).map_err(|e| unopenable(path, &e))?;
    if !meta.is_file() {
        return Err(WireError::new(
            ErrorKind::Forbidden,
            format!("`{path}` is not a regular file; this reads files, not streams"),
        ));
    }
    let total = meta.len();

    let mut file = std::fs::File::open(&resolved).map_err(|e| unopenable(path, &e))?;

    // A size of zero is not evidence of an empty file. A pseudo-file reports one
    // and still has content — `/proc/loadavg` is a regular file of size 0 — so
    // trusting it would answer an empty slice marked `eof`, which is
    // indistinguishable from a file that really is empty. The only trustworthy
    // end is a read that returns nothing, so these are read past the offset and
    // ended by the read itself. A wait means nothing to them either: their
    // length cannot be asked for, so they are read now.
    if total == 0 {
        return read_now(&mut file, path, offset, len);
    }

    // Past the end is where a follower waits. A copier (no wait) is told the
    // file ended here, which is how it discovers it is done.
    if offset >= total {
        if wait_ms == 0 {
            return Ok(Slice {
                bytes: Vec::new(),
                next: total,
                eof: true,
                waited: false,
            });
        }
        if !wait_until_grown(&file, offset, wait_ms)? {
            // The wait expired with nothing new. This is an answer, not a
            // failure: the file is still there, it simply did not grow.
            return Ok(Slice {
                bytes: Vec::new(),
                next: offset,
                eof: true,
                waited: true,
            });
        }
    }

    use std::io::{Seek, SeekFrom};
    file.seek(SeekFrom::Start(offset))
        .map_err(|e| WireError::internal(format!("cannot seek in `{path}`: {e}")))?;

    read_now(&mut file, path, offset, len)
}

/// Reads at the current position and reports where it started.
fn read_now(
    file: &mut std::fs::File,
    path: &str,
    offset: u64,
    len: u32,
) -> Result<Slice, WireError> {
    let mut buffer = vec![0u8; len as usize];
    let filled = fill(file, &mut buffer, path)?;
    buffer.truncate(filled);
    let next = offset + filled as u64;

    // A short read is the only evidence of the end: a size from `stat` is not,
    // for a `/proc` entry, and it may also grow between the two calls.
    Ok(Slice {
        bytes: buffer,
        next,
        eof: filled < len as usize,
        waited: false,
    })
}

/// The wait a caller actually gets, with the daemon's cap applied.
///
/// A pure function so the cap can be tested without serving the thirty seconds
/// it bounds: a test that slept through it would cost the suite half a minute
/// and still only prove that sleeping works.
fn bounded_wait(wait_ms: u32) -> std::time::Duration {
    std::time::Duration::from_millis(u64::from(wait_ms.min(MAX_WAIT_MS)))
}

/// Sleeps until `file` holds more than `offset` bytes, or the wait expires.
///
/// The growth is asked of the **open handle**, not of the path. If the file is
/// rotated the handle keeps describing the file being read, so a follower is
/// never told to expect bytes that belong to a different file; it sees the old
/// file stop growing, which is the truth about what it is reading.
///
/// The deadline is recomputed against the cap here rather than trusted from the
/// caller, so an oversized `wait_ms` cannot outlive [`MAX_WAIT_MS`].
fn wait_until_grown(file: &std::fs::File, offset: u64, wait_ms: u32) -> Result<bool, WireError> {
    let deadline = std::time::Instant::now() + bounded_wait(wait_ms);

    loop {
        let now = std::time::Instant::now();
        if now >= deadline {
            return Ok(false);
        }
        std::thread::sleep(std::time::Duration::from_millis(POLL_MS).min(deadline - now));

        let total = file
            .metadata()
            .map_err(|e| WireError::internal(format!("cannot stat the file being read: {e}")))?
            .len();
        if total > offset {
            return Ok(true);
        }
    }
}

/// Fills `buffer` from `file`, returning how much was written.
///
/// `read` may return less than asked for without being at the end, so this
/// loops until the buffer is full or the file really ends. A short read is the
/// only evidence of the end: a size from `stat` is not, for a `/proc` entry.
fn fill(file: &mut std::fs::File, buffer: &mut [u8], path: &str) -> Result<usize, WireError> {
    use std::io::Read;

    let mut filled = 0;
    while filled < buffer.len() {
        match file.read(&mut buffer[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(WireError::internal(format!("cannot read `{path}`: {e}"))),
        }
    }
    Ok(filled)
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
    let real = joined.canonicalize().map_err(|e| unopenable(path, &e))?;

    if !real.starts_with(&root) {
        return Err(forbidden(path, "resolves outside the readable root"));
    }
    Ok(real)
}

/// The one wording for "this path cannot be reached", so `stat`, `open` and
/// `canonicalize` cannot describe the same failure three different ways.
fn unopenable(path: &str, error: &std::io::Error) -> WireError {
    WireError::new(
        ErrorKind::NotFound,
        format!("cannot open `{path}`: {error}"),
    )
}

fn forbidden(path: &str, why: &str) -> WireError {
    WireError::new(ErrorKind::Forbidden, format!("`{path}` {why}"))
}

#[cfg(test)]
mod tests;
