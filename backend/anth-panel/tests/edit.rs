//! Text saving, exercised through the real `FileService` on a temporary tree.

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anth_panel::config::FilesConfig;
use anth_panel::files::FileService;
use anth_proto::FileErrorKind;

struct Fixture {
    base: PathBuf,
    service: FileService,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base =
            std::env::temp_dir().join(format!("anth-edit-{name}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&base).unwrap();
        let service = FileService::new(&FilesConfig {
            root: base.to_string_lossy().into_owned(),
            write: true,
            ..FilesConfig::default()
        })
        .unwrap();
        Self { base, service }
    }

    fn write(&self, name: &str, body: &str) -> PathBuf {
        let path = self.base.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, body).unwrap();
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

#[test]
fn saving_replaces_the_contents_and_returns_the_new_size() {
    let fixture = Fixture::new("save");
    fixture.write("notes.txt", "old");

    let modified_ms = fixture
        .service
        .write_text("/notes.txt", "new contents", None)
        .unwrap();

    assert_eq!(
        fs::read_to_string(fixture.base.join("notes.txt")).unwrap(),
        "new contents"
    );
    assert!(modified_ms > 0, "the new mtime must be reported");
}

#[test]
fn saving_leaves_no_temporary_file_behind() {
    let fixture = Fixture::new("no-temp");
    fixture.write("a.txt", "x");

    fixture.service.write_text("/a.txt", "y", None).unwrap();

    let leftovers: Vec<String> = fs::read_dir(&fixture.base)
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(".anth-upload-"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "temporary files remain: {leftovers:?}"
    );
}

#[test]
fn a_stale_precondition_is_refused_and_the_file_is_untouched() {
    let fixture = Fixture::new("conflict");
    let path = fixture.write("shared.txt", "first");

    let loaded = fixture
        .service
        .write_text("/shared.txt", "second", None)
        .unwrap();
    // Another writer wins the race.
    std::thread::sleep(Duration::from_millis(5));
    fs::write(&path, "someone else").unwrap();
    let stale = loaded.saturating_sub(1);

    let error = fixture
        .service
        .write_text("/shared.txt", "mine", Some(stale))
        .unwrap_err();

    assert_eq!(error.kind, FileErrorKind::Conflict);
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "someone else",
        "a refused save must not touch the file"
    );
}

#[test]
fn a_current_precondition_is_accepted() {
    let fixture = Fixture::new("fresh");
    fixture.write("a.txt", "one");
    let modified = fixture.service.write_text("/a.txt", "two", None).unwrap();
    // Same-millisecond saves are the common case in a UI, so the timestamp must
    // be usable immediately.
    fixture
        .service
        .write_text("/a.txt", "three", Some(modified))
        .unwrap();

    assert_eq!(
        fs::read_to_string(fixture.base.join("a.txt")).unwrap(),
        "three"
    );
}

#[test]
fn a_binary_payload_is_refused() {
    let fixture = Fixture::new("binary");
    fixture.write("a.txt", "text");

    let error = fixture
        .service
        .write_text("/a.txt", "bad\0byte", None)
        .unwrap_err();

    assert_eq!(error.kind, FileErrorKind::InvalidPath);
    assert_eq!(
        fs::read_to_string(fixture.base.join("a.txt")).unwrap(),
        "text"
    );
}

#[test]
fn writing_to_a_directory_is_refused() {
    let fixture = Fixture::new("dir");
    fs::create_dir_all(fixture.base.join("folder")).unwrap();

    let error = fixture
        .service
        .write_text("/folder", "x", None)
        .unwrap_err();

    assert_eq!(error.kind, FileErrorKind::IsADirectory);
}

#[test]
fn a_missing_file_is_not_silently_created() {
    // Creating a file is `upload`; an editor save must not turn a deleted file
    // back into an empty one.
    let fixture = Fixture::new("missing");

    let error = fixture
        .service
        .write_text("/gone.txt", "x", None)
        .unwrap_err();

    assert_eq!(error.kind, FileErrorKind::NotFound);
    assert!(!fixture.base.join("gone.txt").exists());
}

#[test]
fn writing_is_gated_when_files_write_is_off() {
    let base = std::env::temp_dir().join(format!(
        "anth-edit-gate-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&base).unwrap();
    fs::write(base.join("a.txt"), "x").unwrap();
    let service = FileService::new(&FilesConfig {
        root: base.to_string_lossy().into_owned(),
        write: false,
        ..FilesConfig::default()
    })
    .unwrap();

    let error = service.write_text("/a.txt", "y", None).unwrap_err();

    assert_eq!(error.kind, FileErrorKind::Disabled);
    let _ = fs::remove_dir_all(&base);
}

#[test]
fn contents_are_written_as_utf8_byte_for_byte() {
    let fixture = Fixture::new("utf8");
    fixture.write("a.txt", "x");
    let body = "línea uno\nlínea dos ✓\n";

    fixture.service.write_text("/a.txt", body, None).unwrap();

    assert_eq!(
        fs::read_to_string(fixture.base.join("a.txt")).unwrap(),
        body
    );
}

#[test]
fn a_symlink_inside_the_root_is_written_through_rather_than_replaced() {
    // Editing a symlinked config file should update the target, which is what
    // every editor does; replacing the link would silently detach it.
    let fixture = Fixture::new("symlink");
    fixture.write("real.txt", "before");
    std::os::unix::fs::symlink(fixture.base.join("real.txt"), fixture.base.join("link.txt"))
        .unwrap();

    fixture
        .service
        .write_text("/link.txt", "after", None)
        .unwrap();

    // `host_existing` canonicalises, so the save lands on the target and the
    // link survives. Replacing the link would detach it from the file it names.
    assert!(
        fs::symlink_metadata(fixture.base.join("link.txt"))
            .unwrap()
            .file_type()
            .is_symlink(),
        "the symlink was replaced by a regular file"
    );
    assert_eq!(
        fs::read_to_string(fixture.base.join("real.txt")).unwrap(),
        "after"
    );
}
