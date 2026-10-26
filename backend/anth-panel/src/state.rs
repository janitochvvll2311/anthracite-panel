//! Atomic, locked writes for operator-editable files.
//!
//! The write path is the security-sensitive one: a temp file in the same
//! directory, the final mode from creation, `fsync`, then `rename`. A failed
//! write leaves the original byte-for-byte.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use libc::{LOCK_EX, flock};

/// Held for the duration of a read-modify-write.
pub struct FileLock {
    file: File,
}

impl FileLock {
    /// Acquires an exclusive advisory lock on `<path>.lock`.
    pub fn acquire(path: &Path) -> Result<Self, String> {
        let lock_path = lock_path(path);
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("creating {}: {error}", parent.display()))?;
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(&lock_path)
            .map_err(|error| format!("opening {}: {error}", lock_path.display()))?;
        let result = unsafe { flock(file.as_raw_fd(), LOCK_EX) };
        if result != 0 {
            return Err(format!(
                "locking {}: {}",
                lock_path.display(),
                io::Error::last_os_error()
            ));
        }
        Ok(Self { file })
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = unsafe { flock(self.file.as_raw_fd(), libc::LOCK_UN) };
    }
}

use std::os::unix::io::AsRawFd;

fn lock_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".lock");
    path.with_file_name(name)
}

fn temp_path(path: &Path) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".tmp.{}.{nanos}", std::process::id()));
    path.with_file_name(name)
}

/// Writes `bytes` to `path` atomically.
pub fn write_atomic(path: &Path, bytes: &[u8], mode: u32) -> Result<(), String> {
    let parent = path.parent().filter(|value| !value.as_os_str().is_empty());
    if let Some(parent) = parent {
        fs::create_dir_all(parent)
            .map_err(|error| format!("creating {}: {error}", parent.display()))?;
    }

    let temp = temp_path(path);
    let result = (|| -> io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, path)?;
        if let Some(parent) = parent {
            File::open(parent)?.sync_all()?;
        }
        Ok(())
    })();

    if let Err(error) = result {
        let _ = fs::remove_file(&temp);
        return Err(format!("writing {}: {error}", path.display()));
    }
    Ok(())
}

/// Reads a file, treating a missing file as an empty string.
pub fn read_or_empty(path: &Path) -> Result<String, String> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(format!("reading {}: {error}", path.display())),
    }
}

/// Runs `edit` under the file lock and atomically writes its result.
///
/// `edit` receives the current text and returns the new text. A parse or
/// validation error is returned without touching the file.
pub fn update<F>(path: &Path, mode: u32, edit: F) -> Result<(), String>
where
    F: FnOnce(&str) -> Result<String, String>,
{
    let _lock = FileLock::acquire(path)?;
    let current = read_or_empty(path)?;
    let next = edit(&current)?;
    write_atomic(path, next.as_bytes(), mode)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = PathBuf::from("/tmp/anthracite-panel/tests")
            .join(format!("{name}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&root).expect("create scratch");
        root
    }

    #[test]
    fn atomic_write_sets_the_mode_and_leaves_no_temp() {
        use std::os::unix::fs::MetadataExt;

        let root = scratch("atomic");
        let path = root.join("config.toml");
        write_atomic(&path, b"version = 1\n", 0o600).expect("write");
        assert_eq!(fs::read_to_string(&path).unwrap(), "version = 1\n");
        let mode = fs::metadata(&path).unwrap().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let names: Vec<_> = fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("config.toml")]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_failed_edit_leaves_the_original_file() {
        use std::os::unix::fs::PermissionsExt;

        let root = scratch("atomic-refuse");
        let path = root.join("config.toml");
        write_atomic(&path, b"version = 1\n", 0o644).expect("write");
        let error =
            update(&path, 0o644, |_| Err::<String, _>("bad value".into())).expect_err("refused");
        assert_eq!(error, "bad value");
        assert_eq!(fs::read_to_string(&path).unwrap(), "version = 1\n");
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o644
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn update_sees_the_current_text_and_persists() {
        let root = scratch("atomic-update");
        let path = root.join("state.toml");
        write_atomic(&path, b"a = 1\n", 0o600).expect("write");
        update(&path, 0o600, |text| {
            assert_eq!(text, "a = 1\n");
            Ok("a = 2\n".to_string())
        })
        .expect("update");
        assert_eq!(fs::read_to_string(&path).unwrap(), "a = 2\n");
        fs::remove_dir_all(root).unwrap();
    }
}
