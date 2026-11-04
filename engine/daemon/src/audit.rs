//! The audit log file (§6.8): one JSON object per line, append-only.
//!
//! The state tree owns the directory and the file's first creation. This module
//! only appends: no rewrite, no rotation, no truncation. A line is written with
//! one `write_all` and synced, because the record of an approval matters more
//! than the cost of one `fsync` on an operation that happens by hand.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use anth_contract::{AuditEntry, Error};

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
        self.check_file()?;
        let mut line = entry.to_line()?;
        line.push('\n');
        let mut file = OpenOptions::new()
            .append(true)
            .create(true)
            .mode(FILE_MODE)
            // A symlink where the log should be is refused rather than followed.
            .custom_flags(libc::O_NOFOLLOW)
            .open(&self.path)
            .map_err(|error| self.describe("opening", error))?;
        file.write_all(line.as_bytes())
            .and_then(|()| file.sync_data())
            .map_err(|error| self.describe("writing", error))
    }

    /// Every entry, in the order it was written. A missing file is an empty log.
    pub fn entries(&self) -> Result<Vec<AuditEntry>, Error> {
        let text = match fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(self.describe("reading", error)),
        };
        let mut entries = Vec::new();
        for (index, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            entries.push(AuditEntry::parse_line(line).map_err(|error| {
                Error::invalid(format!("{}:{}: {error}", self.path.display(), index + 1))
            })?);
        }
        Ok(entries)
    }

    fn check_file(&self) -> Result<(), Error> {
        match fs::symlink_metadata(&self.path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(Error::invalid(format!(
                        "{} is a symlink; the state tree must be real files",
                        self.path.display()
                    )));
                }
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
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(self.describe("inspecting", error)),
        }
    }

    fn describe(&self, what: &str, error: std::io::Error) -> Error {
        if error.kind() == std::io::ErrorKind::PermissionDenied {
            Error::denied(format!("{what} {}: {error}", self.path.display()))
        } else {
            Error::internal(format!("{what} {}: {error}", self.path.display()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anth_contract::{AuditAction, Outcome};

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
