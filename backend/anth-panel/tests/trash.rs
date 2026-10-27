//! Trash contract tests, ported from v0.1.
//!
//! Every test pins the mount table to its own temporary directory, so the trash
//! lands inside the fixture instead of the running user's real
//! `$XDG_DATA_HOME/Trash`.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anth_panel::files::{FileService, TrashPaths};
use anth_wire::ErrorKind;

struct Fixture {
    root: PathBuf,
    data_home: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base =
            std::env::temp_dir().join(format!("anth-trash-{name}-{}-{nanos}", std::process::id()));
        let root = base.join("root");
        let data_home = base.join("data");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&data_home).unwrap();
        Self { root, data_home }
    }

    fn service(&self, trash: bool) -> FileService {
        self.service_with_limit(trash, 1024 * 1024 * 1024)
    }

    fn service_with_limit(&self, trash: bool, max_trash_bytes: u64) -> FileService {
        FileService::new(&self.root, true, 100, 64, 1024)
            .unwrap()
            .with_trash(trash, max_trash_bytes)
            .with_trash_paths(TrashPaths {
                topdirs: vec![self.root.clone()],
                home: self.root.clone(),
                data_home: self.data_home.clone(),
            })
    }

    fn write(&self, path: &str, bytes: &[u8]) {
        let host = self.root.join(path.trim_start_matches('/'));
        if let Some(parent) = host.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(host, bytes).unwrap();
    }

    fn host(&self, path: &str) -> PathBuf {
        self.root.join(path.trim_start_matches('/'))
    }

    fn data_home(&self) -> &Path {
        &self.data_home
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(parent) = self.root.parent() {
            let _ = fs::remove_dir_all(parent);
        }
    }
}

#[test]
fn trashing_moves_the_entry_out_of_the_directory() {
    let fixture = Fixture::new("move");
    fixture.write("/notes.txt", b"hello");
    let service = fixture.service(true);

    let (path, id) = service.trash("/notes.txt").unwrap();

    assert_eq!(path, "notes.txt");
    assert_eq!(id, "notes.txt");
    assert!(!fixture.host("/notes.txt").exists());
    assert_eq!(
        service.list("/", &Default::default()).unwrap().items.len(),
        0
    );
}

#[test]
fn trashing_is_refused_when_the_feature_is_off() {
    let fixture = Fixture::new("disabled");
    fixture.write("/notes.txt", b"hello");
    let service = fixture.service(false);

    let error = service.trash("/notes.txt").unwrap_err();

    assert_eq!(error.code, "trash_disabled");
    assert_eq!(error.kind, ErrorKind::Forbidden);
    assert!(fixture.host("/notes.txt").exists());
}

#[test]
fn trashing_needs_write_permission_too() {
    let fixture = Fixture::new("nowrite");
    fixture.write("/notes.txt", b"hello");
    let service = FileService::new(&fixture.root, false, 100, 64, 1024)
        .unwrap()
        .with_trash(true, 1024 * 1024 * 1024)
        .with_trash_paths(TrashPaths {
            topdirs: vec![fixture.root.clone()],
            home: fixture.root.clone(),
            data_home: fixture.data_home().to_path_buf(),
        });

    let error = service.trash("/notes.txt").unwrap_err();

    assert_eq!(error.code, "write_disabled");
    assert!(fixture.host("/notes.txt").exists());
}

#[test]
fn trashing_a_missing_entry_reports_not_found() {
    let fixture = Fixture::new("missing");
    let service = fixture.service(true);

    assert_eq!(
        service.trash("/nope.txt").unwrap_err().kind,
        ErrorKind::NotFound
    );
}

#[test]
fn trashing_refuses_paths_that_escape_the_root() {
    let fixture = Fixture::new("escape");
    let service = fixture.service(true);

    assert_eq!(
        service.trash("/../etc/passwd").unwrap_err().code,
        "path_escape"
    );
}

#[test]
fn a_trashed_directory_is_restored_with_its_contents() {
    let fixture = Fixture::new("restore-dir");
    fixture.write("/project/a.txt", b"a");
    fixture.write("/project/sub/b.txt", b"b");
    let service = fixture.service(true);

    let (_, id) = service.trash("/project").unwrap();
    assert!(!fixture.host("/project").exists());

    service.restore(&id, false).unwrap();

    assert_eq!(fs::read(fixture.host("/project/a.txt")).unwrap(), b"a");
    assert_eq!(fs::read(fixture.host("/project/sub/b.txt")).unwrap(), b"b");
}

#[test]
fn restoring_refuses_to_overwrite_an_existing_entry() {
    let fixture = Fixture::new("restore-conflict");
    fixture.write("/notes.txt", b"first");
    let service = fixture.service(true);
    let (_, id) = service.trash("/notes.txt").unwrap();

    fixture.write("/notes.txt", b"second");

    let error = service.restore(&id, false).unwrap_err();
    assert_eq!(error.code, "exists");
    assert_eq!(fs::read(fixture.host("/notes.txt")).unwrap(), b"second");

    service.restore(&id, true).unwrap();
    assert_eq!(fs::read(fixture.host("/notes.txt")).unwrap(), b"first");
}

#[test]
fn restoring_over_a_directory_with_force_is_refused() {
    let fixture = Fixture::new("restore-isdir");
    fixture.write("/notes.txt", b"file");
    let service = fixture.service(true);
    let (_, id) = service.trash("/notes.txt").unwrap();
    fs::create_dir_all(fixture.host("/notes.txt")).unwrap();

    let error = service.restore(&id, true).unwrap_err();

    assert_eq!(error.kind, ErrorKind::Invalid);
}

#[test]
fn a_second_delete_of_the_same_name_gets_a_numbered_identifier() {
    let fixture = Fixture::new("collision");
    fixture.write("/notes.txt", b"first");
    let service = fixture.service(true);

    let (_, first) = service.trash("/notes.txt").unwrap();
    fixture.write("/notes.txt", b"second");
    let (_, second) = service.trash("/notes.txt").unwrap();

    assert_eq!(first, "notes.txt");
    assert_eq!(second, "notes.txt.1");
    assert_eq!(service.trash_listing().unwrap().entries.len(), 2);
}

#[test]
fn the_listing_reports_where_an_entry_came_from() {
    let fixture = Fixture::new("listing");
    fixture.write("/reports/q1.pdf", b"pdf-bytes");
    let service = fixture.service(true);
    service.trash("/reports/q1.pdf").unwrap();

    let listing = service.trash_listing().unwrap();

    assert_eq!(listing.total, 1);
    assert!(listing.enabled);
    let entry = &listing.entries[0];
    assert_eq!(entry.original_path, "/reports/q1.pdf");
    assert_eq!(entry.name, "q1.pdf");
    assert_eq!(entry.size, 9);
    assert!(entry.deleted_at.is_some());
    assert!(entry.location.ends_with("Trash"));
}

#[test]
fn emptying_the_trash_removes_files_and_metadata() {
    let fixture = Fixture::new("empty");
    fixture.write("/a.txt", b"a");
    fixture.write("/b.txt", b"b");
    let service = fixture.service(true);
    service.trash("/a.txt").unwrap();
    service.trash("/b.txt").unwrap();

    let removed = service.empty_trash().unwrap();

    assert_eq!(removed, 2);
    let listing = service.trash_listing().unwrap();
    assert_eq!(listing.total, 0);
    assert_eq!(listing.used_bytes, 0);
}

#[test]
fn a_hostile_trashinfo_cannot_restore_outside_the_root() {
    let fixture = Fixture::new("hostile");
    let service = fixture.service(true);
    let files = fixture.data_home().join("Trash/files");
    let info = fixture.data_home().join("Trash/info");
    fs::create_dir_all(&files).unwrap();
    fs::create_dir_all(&info).unwrap();
    fs::write(files.join("evil.txt"), b"payload").unwrap();
    fs::write(
        info.join("evil.txt.trashinfo"),
        "[Trash Info]\nPath=/../../etc/evil.txt\nDeletionDate=2024-01-01T00:00:00\n",
    )
    .unwrap();

    let error = service.restore("evil.txt", false).unwrap_err();

    assert_eq!(error.code, "invalid_path");
    assert!(files.join("evil.txt").exists());
}

#[test]
fn a_malformed_trashinfo_is_skipped_without_hiding_the_others() {
    let fixture = Fixture::new("malformed");
    fixture.write("/good.txt", b"good");
    let service = fixture.service(true);
    service.trash("/good.txt").unwrap();
    fs::write(
        fixture.data_home().join("Trash/info/broken.txt.trashinfo"),
        "[Trash Info]\nDeletionDate=2024-01-01T00:00:00\n",
    )
    .unwrap();

    let listing = service.trash_listing().unwrap();

    assert_eq!(listing.total, 1);
    assert_eq!(listing.entries[0].id, "good.txt");
}

#[test]
fn restore_rejects_identifiers_that_are_paths() {
    let fixture = Fixture::new("bad-id");
    let service = fixture.service(true);

    for id in ["../etc/passwd", "a/b", "", ".."] {
        assert_eq!(
            service.restore(id, false).unwrap_err().code,
            "invalid_path",
            "`{id}` must be refused before any lookup"
        );
    }
    assert_eq!(
        service.restore("absent.txt", false).unwrap_err().kind,
        ErrorKind::NotFound
    );
}

#[test]
fn retention_drops_the_oldest_entries_first() {
    let fixture = Fixture::new("retention");
    let service = fixture.service_with_limit(true, 12);

    fixture.write("/a.txt", b"aaaaaaaaaa");
    service.trash("/a.txt").unwrap();
    fixture.write("/b.txt", b"bbbbbbbbbb");
    service.trash("/b.txt").unwrap();

    let listing = service.trash_listing().unwrap();

    assert!(
        listing.total <= 1,
        "retention must have dropped an entry, found {}",
        listing.total
    );
    if listing.total == 1 {
        assert_eq!(listing.entries[0].id, "b.txt");
    }
}
