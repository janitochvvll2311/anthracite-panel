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

#[cfg(test)]
mod tests {
    use super::*;

    /// A file inside a directory this fixture owns, both removed on drop.
    ///
    /// The owned directory matters: cleanup must never reach beyond it. An
    /// earlier version kept the file directly in `temp_dir()`, so its parent
    /// was the temp root itself and the drop deleted every other process's
    /// files under `/tmp`.
    struct TempFile {
        root: PathBuf,
        file: PathBuf,
    }

    impl TempFile {
        fn new(label: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let root = std::env::temp_dir().join(format!(
                "anth-history-{label}-{}-{nanos}",
                std::process::id()
            ));
            std::fs::create_dir_all(&root).expect("creating the fixture directory");
            Self {
                file: root.join("history"),
                root,
            }
        }

        fn path(&self) -> &Path {
            &self.file
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn history(lines: &[&str]) -> History {
        let mut history = History::new();
        for line in lines {
            history.push(line);
        }
        history
    }

    #[test]
    fn a_missing_file_is_an_empty_history() {
        let file = TempFile::new("missing");

        assert!(load(file.path()).entries().is_empty());
    }

    /// The round trip is the whole feature: what you typed yesterday is what Up
    /// finds today.
    #[test]
    fn the_round_trip_preserves_the_order() {
        let file = TempFile::new("round-trip");
        let original = history(&["first", "second", "third"]);

        save(file.path(), &original).expect("writing");
        let loaded = load(file.path());

        assert_eq!(loaded.entries(), ["third", "second", "first"]);
    }

    /// On disk the oldest line comes first, so `tail` shows the newest — the
    /// convention every other history file follows.
    #[test]
    fn the_file_is_written_oldest_first() {
        let file = TempFile::new("oldest-first");
        save(file.path(), &history(&["first", "second"])).expect("writing");

        let text = std::fs::read_to_string(file.path()).expect("reading");

        assert_eq!(text, "first\nsecond\n");
    }

    #[test]
    fn blank_lines_are_not_loaded_as_commands() {
        let file = TempFile::new("blank");
        std::fs::write(file.path(), "one\n\n   \ntwo\n").expect("writing a fixture");

        let loaded = load(file.path());

        assert_eq!(loaded.entries(), ["two", "one"]);
    }

    /// A history written by an older version with no trailing newline must still
    /// load, which is why the reader uses `lines()` and not `split_inclusive`.
    #[test]
    fn a_file_without_a_final_newline_loads() {
        let file = TempFile::new("no-newline");
        std::fs::write(file.path(), "one\ntwo").expect("writing a fixture");

        assert_eq!(load(file.path()).entries(), ["two", "one"]);
    }

    /// The parent directory is created on demand: a fresh account has no
    /// `~/.local/state/anth-panel`.
    #[test]
    fn saving_creates_the_parent_directory() {
        let root = TempFile::new("mkdir");
        let nested = root.path().join("a").join("b").join("history");

        save(&nested, &history(&["one"])).expect("writing into a missing directory");
        assert_eq!(load(&nested).entries(), ["one"]);
    }

    /// Replacing the file rather than truncating it means a failed write cannot
    /// leave a half-history behind.
    #[test]
    fn saving_replaces_a_previous_file() {
        let file = TempFile::new("replace");
        save(file.path(), &history(&["one", "two"])).expect("first write");

        save(file.path(), &history(&["three"])).expect("second write");

        assert_eq!(load(file.path()).entries(), ["three"]);
    }

    /// Nothing is left behind when the rename succeeds.
    #[test]
    fn no_temporary_file_survives_a_successful_save() {
        let file = TempFile::new("no-temp");
        save(file.path(), &history(&["one"])).expect("writing");

        let temporary = file
            .path()
            .with_extension(format!("tmp{}", std::process::id()));
        assert!(!temporary.exists());
    }

    /// `ANTH_HISTORY` wins, which is what lets a test or a user redirect it.
    #[test]
    fn the_path_honours_the_explicit_override() {
        // SAFETY: the variable is read only by `path`, and this test is the only
        // one that sets it.
        unsafe {
            std::env::set_var("ANTH_HISTORY", "/tmp/explicit-history");
        }

        let resolved = path();

        unsafe {
            std::env::remove_var("ANTH_HISTORY");
        }
        assert_eq!(resolved, Some(PathBuf::from("/tmp/explicit-history")));
    }

    /// With no override, the fallback follows XDG rather than dumping state in
    /// the home directory.
    #[test]
    fn the_fallback_is_under_the_state_directory() {
        // SAFETY: these variables are read only by `path`.
        unsafe {
            std::env::remove_var("ANTH_HISTORY");
            std::env::set_var("XDG_STATE_HOME", "/tmp/state-home");
        }

        let resolved = path();

        unsafe {
            std::env::remove_var("XDG_STATE_HOME");
        }
        assert_eq!(
            resolved,
            Some(PathBuf::from("/tmp/state-home/anth-panel/history"))
        );
    }
}
