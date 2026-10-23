//! Command history on disk, so it survives the session.
//!
//! One command per line is the format `HISTFILE` has used for decades, which
//! means `$HISTFILE` conventions, `shuf` and every `grep` already apply. It also
//! degrades well: a corrupt line is one bad entry, not a lost file.
//!
//! The file is written whole and replaced by rename, so a crash mid-write leaves
//! the previous history intact instead of a truncated one. Losing history is
//! annoying; a half-written file that refuses to parse on every start is worse.

use std::io::Write;
use std::path::{Path, PathBuf};

use super::editor::History;

/// How many lines the file keeps. A prompt is not an audit log, and an unbounded
/// file would grow forever in a home directory nobody prunes.
const MAX_ENTRIES: usize = 1000;

/// Where the history lives.
///
/// `ANTH_HISTORY` overrides it so a test can point at a temporary file and a
/// user can keep the panel's history out of their home. Otherwise it follows
/// `XDG_STATE_HOME`, then `~/.local/state`, per the XDG base directory spec:
/// this is state, not configuration, so it does not belong in `~/.config`.
pub fn path() -> Option<PathBuf> {
    if let Ok(explicit) = std::env::var("ANTH_HISTORY")
        && !explicit.is_empty()
    {
        return Some(PathBuf::from(explicit));
    }
    if let Ok(state) = std::env::var("XDG_STATE_HOME")
        && !state.is_empty()
    {
        return Some(PathBuf::from(state).join("anth-panel").join("history"));
    }
    let home = std::env::var("HOME").ok().filter(|home| !home.is_empty())?;
    Some(
        PathBuf::from(home)
            .join(".local")
            .join("state")
            .join("anth-panel")
            .join("history"),
    )
}

/// Reads the history at `path`, newest last on disk and newest first in memory.
///
/// A missing or unreadable file is an empty history, not an error: a first run
/// has no file, and a prompt that refused to start over one would be unusable.
/// Lines are kept in file order, and `History` reverses them, so the file stays
/// readable by tools that expect oldest-first.
pub fn load(path: &Path) -> History {
    let Ok(text) = std::fs::read_to_string(path) else {
        return History::new();
    };

    let mut history = History::new();
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    // `push` inserts at the front, so feeding oldest-first leaves the newest at
    // the front of the in-memory list.
    for line in lines {
        history.push(line);
    }
    history
}

/// Writes `history` to `path`, oldest first, keeping at most [`MAX_ENTRIES`].
///
/// Errors are returned for the caller to report once; a prompt keeps working
/// without persistence, so this is never fatal.
pub fn save(path: &Path, history: &History) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let newest_first = history.entries();
    let kept = newest_first.len().min(MAX_ENTRIES);

    // Write beside the target and rename, so a reader never sees a partial file.
    let temporary = path.with_extension(format!("tmp{}", std::process::id()));
    {
        let mut file = std::fs::File::create(&temporary)?;
        // Newest last, matching every other history file.
        for entry in newest_first.iter().take(kept).rev() {
            writeln!(file, "{entry}")?;
        }
        file.flush()?;
    }

    match std::fs::rename(&temporary, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = std::fs::remove_file(&temporary);
            Err(error)
        }
    }
}
