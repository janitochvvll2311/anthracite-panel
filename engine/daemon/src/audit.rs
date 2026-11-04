//! The audit log file (§6.8): one JSON object per line, append-only.
//!
//! The state tree owns the directory and the file's first creation. This module
//! only appends: no rewrite, no rotation, no truncation. A line is written with
//! one `write_all` and synced, because the record of an approval matters more
//! than the cost of one `fsync` on an operation that happens by hand.
//!
//! Two guards keep a crash or a full disk from corrupting the log: a failed
//! write is rolled back to the previous length, and an append refuses a file
//! that already ends in a partial line instead of gluing a new entry onto it.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use anth_contract::{AuditEntry, Error, ErrorKind};

pub const AUDIT_FILE: &str = "audit.jsonl";
const FILE_MODE: u32 = 0o600;

#[derive(Debug, Clone)]
pub struct AuditLog {
    path: PathBuf,
}

impl AuditLog {
    pub fn new(state: &Path) -> Self {
        Self {
            path: state.join(AUDIT_FILE),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Appends one entry. The entry is validated before the file is touched, so
    /// a rejected action leaves no half-line behind.
    pub fn append(&self, entry: &AuditEntry) -> Result<(), Error> {
        entry.validate()?;
        let mut line = entry.to_line()?;
        line.push('\n');
        let mut file = self.open(true)?;
        let before = file
            .metadata()
            .map_err(|error| self.describe("inspecting", error))?
            .len();
        if !ends_with_newline(&mut file, before).map_err(|error| self.describe("reading", error))? {
            return Err(Error::invalid(format!(
                "{} ends with a partial line; repair or remove it before appending",
                self.path.display()
            )));
        }
        if let Err(error) = file.write_all(line.as_bytes()) {
            // The engine is the only writer, so the length before the write is
            // the only state to restore. Better an unchanged log than a line
            // that cannot be parsed.
            let _ = file.set_len(before);
            return Err(self.describe("writing", error));
        }
        file.sync_data()
            .map_err(|error| self.describe("syncing", error))
    }

    /// Every entry, refusing the whole read when a line cannot be parsed.
    pub fn entries(&self) -> Result<Vec<AuditEntry>, Error> {
        let (entries, problems) = self.scan()?;
        if let Some(problem) = problems.into_iter().next() {
            return Err(Error::invalid(problem));
        }
        Ok(entries)
    }

    /// What could be read, plus one problem per unparseable line.
    ///
    /// An audit endpoint shows the entries it can and names the lines it cannot;
    /// discarding the whole log because of one bad byte would hide history.
    pub fn scan(&self) -> Result<(Vec<AuditEntry>, Vec<String>), Error> {
        let mut file = match self.open(false) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Ok((Vec::new(), Vec::new()));
            }
            Err(error) => return Err(error),
        };
        let mut text = String::new();
        file.read_to_string(&mut text)
            .map_err(|error| self.describe("reading", error))?;
        let mut entries = Vec::new();
        let mut problems = Vec::new();
        for (index, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            match AuditEntry::parse_line(line) {
                Ok(entry) => entries.push(entry),
                Err(error) => {
                    problems.push(format!("{}:{}: {error}", self.path.display(), index + 1))
                }
            }
        }
        Ok((entries, problems))
    }

    /// Opens the log and checks the **descriptor**, not the path: a file
    /// swapped between a check and the open would otherwise be trusted.
    fn open(&self, create: bool) -> Result<File, Error> {
        let mut options = OpenOptions::new();
        options
            .read(true)
            .append(true)
            // A symlink where the log should be is refused rather than followed.
            .custom_flags(libc::O_NOFOLLOW);
        if create {
            options.create(true).mode(FILE_MODE);
        }
        let file = options.open(&self.path).map_err(|error| {
            if error.raw_os_error() == Some(libc::ELOOP) {
                Error::invalid(format!(
                    "{} is a symlink; the state tree must be real files",
                    self.path.display()
                ))
            } else if error.kind() == std::io::ErrorKind::NotFound {
                Error::not_found(format!("{} is missing", self.path.display()))
            } else {
                self.describe("opening", error)
            }
        })?;
        let metadata = file
            .metadata()
            .map_err(|error| self.describe("inspecting", error))?;
        if !metadata.is_file() {
            return Err(Error::invalid(format!(
                "{} is not a file",
                self.path.display()
            )));
        }
        let mode = metadata.permissions().mode() & 0o777;
        if mode != FILE_MODE {
            return Err(Error::invalid(format!(
                "{} is {mode:04o}; it should be {FILE_MODE:04o}",
                self.path.display()
            )));
        }
        Ok(file)
    }

    fn describe(&self, what: &str, error: std::io::Error) -> Error {
        if error.kind() == std::io::ErrorKind::PermissionDenied {
            Error::denied(format!("{what} {}: {error}", self.path.display()))
        } else {
            Error::internal(format!("{what} {}: {error}", self.path.display()))
        }
    }
}

fn ends_with_newline(file: &mut File, len: u64) -> std::io::Result<bool> {
    if len == 0 {
        return Ok(true);
    }
    file.seek(SeekFrom::Start(len - 1))?;
    let mut last = [0u8; 1];
    file.read_exact(&mut last)?;
    Ok(last[0] == b'\n')
}

#[cfg(test)]
mod tests {
    use super::*;
    use anth_contract::{AuditAction, Outcome};
    use std::fs;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("anthracite-panel")
            .join(format!("anth-audit-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    fn entry(action: AuditAction, actor: &str) -> AuditEntry {
        AuditEntry {
            at: "2026-11-03T20:00:00Z".to_string(),
            actor: actor.to_string(),
            action,
            app: Some("org.example.notes".to_string()),
            result: Outcome::Ok,
            detail: Some(serde_json::json!({"risk": "read"})),
        }
    }

    #[test]
    fn entries_append_in_order() {
        let state = scratch("append");
        let log = AuditLog::new(&state);
        log.append(&entry(AuditAction::AppInstall, "operator-3f2a"))
            .expect("append");
        log.append(&entry(AuditAction::AppApprove, "operator-3f2a"))
            .expect("append");
        let entries = log.entries().expect("entries");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].action, AuditAction::AppInstall);
        assert_eq!(entries[1].action, AuditAction::AppApprove);
        assert_eq!(entries[0].detail, Some(serde_json::json!({"risk": "read"})));
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn appending_never_rewrites_what_was_there() {
        let state = scratch("append-only");
        let log = AuditLog::new(&state);
        log.append(&entry(AuditAction::AppInstall, "operator-3f2a"))
            .expect("append");
        let first = fs::read(log.path()).expect("read");
        log.append(&entry(AuditAction::AppRevoke, "operator-3f2a"))
            .expect("append");
        let second = fs::read(log.path()).expect("read");
        assert!(
            second.starts_with(&first),
            "the first entry must be a byte-for-byte prefix"
        );
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn the_file_is_0600() {
        let state = scratch("mode");
        let log = AuditLog::new(&state);
        log.append(&entry(AuditAction::SessionLogin, "operator-3f2a"))
            .expect("append");
        let mode = fs::metadata(log.path()).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_missing_file_is_an_empty_log() {
        let state = scratch("missing");
        let log = AuditLog::new(&state);
        assert!(log.entries().expect("entries").is_empty());
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_partial_line_is_refused_before_appending() {
        let state = scratch("partial");
        let log = AuditLog::new(&state);
        fs::write(log.path(), b"{\"at\":").expect("write");
        fs::set_permissions(log.path(), fs::Permissions::from_mode(0o600)).expect("chmod");
        let error = log
            .append(&entry(AuditAction::AppInstall, "operator-3f2a"))
            .expect_err("partial line");
        assert!(error.message().contains("partial line"), "{error}");
        assert_eq!(fs::read(log.path()).expect("read"), b"{\"at\":");
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn scan_keeps_the_readable_entries() {
        let state = scratch("scan");
        let log = AuditLog::new(&state);
        log.append(&entry(AuditAction::AppInstall, "operator-3f2a"))
            .expect("append");
        let mut text = fs::read_to_string(log.path()).expect("read");
        text.push_str("{not json}\n");
        fs::write(log.path(), text).expect("write");
        let (entries, problems) = log.scan().expect("scan");
        assert_eq!(entries.len(), 1, "the readable entry survives");
        assert_eq!(problems.len(), 1);
        assert!(log.entries().is_err(), "the strict read still fails");
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_symlink_is_refused() {
        let state = scratch("symlink");
        let outside = state.join("outside.jsonl");
        let log = AuditLog::new(&state);
        std::os::unix::fs::symlink(&outside, log.path()).expect("symlink");
        let error = log
            .append(&entry(AuditAction::AppInstall, "operator-3f2a"))
            .expect_err("symlink");
        assert!(error.message().contains("is a symlink"), "{error}");
        assert!(!outside.exists(), "the link must not be followed");
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_loose_file_is_refused() {
        let state = scratch("loose");
        let log = AuditLog::new(&state);
        fs::write(log.path(), b"").expect("write");
        fs::set_permissions(log.path(), fs::Permissions::from_mode(0o644)).expect("chmod");
        let error = log
            .append(&entry(AuditAction::AppInstall, "operator-3f2a"))
            .expect_err("loose");
        assert!(error.message().contains("0644"), "{error}");
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn an_invalid_entry_is_not_written() {
        let state = scratch("invalid");
        let log = AuditLog::new(&state);
        let mut entry = entry(AuditAction::AppInstall, "operator-3f2a");
        entry.actor = "  ".to_string();
        log.append(&entry).expect_err("invalid");
        assert!(log.entries().expect("entries").is_empty());
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_malformed_line_names_its_number() {
        let state = scratch("malformed");
        let log = AuditLog::new(&state);
        log.append(&entry(AuditAction::AppInstall, "operator-3f2a"))
            .expect("append");
        let mut text = fs::read_to_string(log.path()).expect("read");
        text.push_str("{not json}\n");
        fs::write(log.path(), text).expect("write");
        let error = log.entries().expect_err("malformed");
        assert!(error.message().contains(":2:"), "{error}");
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_denial_without_an_app_is_recorded() {
        let state = scratch("denial");
        let log = AuditLog::new(&state);
        let mut entry = entry(AuditAction::Deny, "system");
        entry.app = None;
        entry.result = Outcome::Denied;
        entry.detail = Some(serde_json::json!({"capability": "anth:store/kv"}));
        log.append(&entry).expect("append");
        assert_eq!(log.entries().expect("entries")[0].result, Outcome::Denied);
        fs::remove_dir_all(&state).expect("cleanup");
    }
}
