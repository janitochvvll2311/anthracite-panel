use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anth_panel::config::FilesConfig;
use anth_panel::files::FileService;
use anth_proto::{FileErrorKind, FileKind};

struct TempRoot(PathBuf);

impl TempRoot {
    fn new(name: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("anth-files-{name}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn service(root: &Path, write: bool) -> FileService {
    FileService::new(&FilesConfig {
        root: root.to_string_lossy().into_owned(),
        write,
        ..FilesConfig::default()
    })
    .unwrap()
}

fn write_file(root: &Path, path: &str, bytes: &[u8]) {
    let path = root.join(path.trim_start_matches('/'));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, bytes).unwrap();
}

#[test]
fn lists_directories_files_hidden_entries_and_symlinks() {
    let root = TempRoot::new("list");
    write_file(root.path(), "alpha.txt", b"alpha");
    write_file(root.path(), "nested/beta.txt", b"beta");
    write_file(root.path(), ".hidden", b"secret");
    std::os::unix::fs::symlink("alpha.txt", root.path().join("link")).unwrap();

    let listing = service(root.path(), false).list("/").unwrap();

    assert_eq!(listing.path, "/");
    assert_eq!(listing.parent, None);
    assert_eq!(listing.total, 4);
    assert!(!listing.truncated);
    assert!(!listing.write_enabled);
    assert_eq!(
        listing
            .entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        [".hidden", "alpha.txt", "link", "nested"]
    );

    let hidden = listing
        .entries
        .iter()
        .find(|entry| entry.name == ".hidden")
        .unwrap();
    assert!(hidden.hidden);
    assert_eq!(hidden.kind, FileKind::File);

    let nested = listing
        .entries
        .iter()
        .find(|entry| entry.name == "nested")
        .unwrap();
    assert_eq!(nested.kind, FileKind::Directory);
    assert_eq!(nested.size, 0);

    let link = listing
        .entries
        .iter()
        .find(|entry| entry.name == "link")
        .unwrap();
    assert_eq!(link.kind, FileKind::Symlink);
    assert_eq!(link.target.as_deref(), Some("alpha.txt"));
}

#[test]
fn truncates_a_listing_at_max_entries() {
    let root = TempRoot::new("truncate");
    write_file(root.path(), "a", b"a");
    write_file(root.path(), "b", b"b");
    write_file(root.path(), "c", b"c");

    let service = FileService::new(&FilesConfig {
        root: root.path().to_string_lossy().into_owned(),
        max_entries: 2,
        ..FilesConfig::default()
    })
    .unwrap();
    let listing = service.list("/").unwrap();

    assert_eq!(listing.entries.len(), 2);
    assert_eq!(listing.total, 3);
    assert!(listing.truncated);
    assert!(listing.warning.unwrap().contains("showing 2 of 3"));
}

#[test]
fn reads_files_in_chunks_and_reports_eof() {
    let root = TempRoot::new("read");
    let bytes = vec![7u8; 600_000];
    write_file(root.path(), "large.bin", &bytes);
    let service = service(root.path(), false);

    let first = service.read("/large.bin", 0, 256 * 1024).unwrap();
    assert_eq!(first.offset, 0);
    assert_eq!(first.size, 600_000);
    assert_eq!(first.bytes.len(), 256 * 1024);
    assert!(!first.eof);

    let second = service.read("/large.bin", 512 * 1024, 256 * 1024).unwrap();
    assert_eq!(second.bytes.len(), 600_000 - 512 * 1024);
    assert!(second.eof);

    let clamped = service.read("/large.bin", 0, u32::MAX).unwrap();
    assert_eq!(clamped.bytes.len(), anth_proto::MAX_FILE_CHUNK as usize);
}

#[test]
fn reads_empty_files_and_rejects_directories() {
    let root = TempRoot::new("empty");
    write_file(root.path(), "empty", b"");
    fs::create_dir(root.path().join("dir")).unwrap();
    let service = service(root.path(), false);

    let chunk = service.read("/empty", 0, 1024).unwrap();
    assert_eq!(chunk.bytes, Vec::<u8>::new());
    assert_eq!(chunk.size, 0);
    assert!(chunk.eof);

    let error = service.read("/dir", 0, 1024).unwrap_err();
    assert_eq!(error.kind, FileErrorKind::IsADirectory);
}

#[test]
fn rejects_parent_components_and_symlink_escapes_for_reads() {
    let root = TempRoot::new("escape");
    let outside = TempRoot::new("outside");
    write_file(outside.path(), "secret", b"outside");
    write_file(root.path(), "inside", b"inside");
    std::os::unix::fs::symlink(outside.path().join("secret"), root.path().join("escape")).unwrap();
    let service = service(root.path(), false);

    assert_eq!(
        service.read("../etc/passwd", 0, 10).unwrap_err().kind,
        FileErrorKind::InvalidPath
    );
    assert_eq!(
        service.read("/escape", 0, 10).unwrap_err().kind,
        FileErrorKind::InvalidPath
    );
    assert_eq!(service.read("/inside", 0, 10).unwrap().bytes, b"inside");
}

#[test]
fn write_operations_are_disabled_by_default() {
    let root = TempRoot::new("disabled");
    let service = service(root.path(), false);

    assert_eq!(
        service.mkdir("/new", false).unwrap_err().kind,
        FileErrorKind::Disabled
    );
    assert_eq!(
        service.delete("/missing", false).unwrap_err().kind,
        FileErrorKind::Disabled
    );
    assert_eq!(
        service.start_upload("/file", false).unwrap_err().kind,
        FileErrorKind::Disabled
    );
}

#[test]
fn creates_renames_and_deletes_entries() {
    let root = TempRoot::new("mutate");
    write_file(root.path(), "file.txt", b"hello");
    let service = service(root.path(), true);

    assert_eq!(service.mkdir("/nested/deep", true).unwrap(), "/nested/deep");
    assert_eq!(
        service.mkdir("/nested/deep", false).unwrap_err().kind,
        FileErrorKind::AlreadyExists
    );

    let (from, to) = service.rename("/file.txt", "/nested/renamed.txt").unwrap();
    assert_eq!(
        (from.as_str(), to.as_str()),
        ("/file.txt", "/nested/renamed.txt")
    );
    assert_eq!(
        service
            .rename("/nested/renamed.txt", "/nested/deep")
            .unwrap_err()
            .kind,
        FileErrorKind::AlreadyExists
    );

    assert_eq!(
        service.delete("/nested", false).unwrap_err().kind,
        FileErrorKind::DirectoryNotEmpty
    );
    assert_eq!(service.delete("/nested", true).unwrap(), "/nested");
    assert_eq!(
        service.delete("/nested", true).unwrap_err().kind,
        FileErrorKind::NotFound
    );
}

#[test]
fn protects_the_virtual_root_from_mutation() {
    let root = TempRoot::new("root-protection");
    let service = service(root.path(), true);

    assert_eq!(
        service.delete("/", false).unwrap_err().kind,
        FileErrorKind::InvalidPath
    );
    assert_eq!(
        service.rename("/", "/other").unwrap_err().kind,
        FileErrorKind::InvalidPath
    );
    assert_eq!(
        service.start_upload("/", false).unwrap_err().kind,
        FileErrorKind::IsADirectory
    );
}

#[test]
fn uploads_atomically_and_cleans_up_aborted_files() {
    let root = TempRoot::new("upload");
    let service = service(root.path(), true);

    let mut upload = service.start_upload("/result.txt", false).unwrap();
    assert_eq!(upload.write_chunk(b"hello ").unwrap(), 6);
    assert_eq!(upload.write_chunk(b"world").unwrap(), 11);
    let written = upload.finish().unwrap();
    assert_eq!(written.path, "/result.txt");
    assert_eq!(written.size, 11);
    assert_eq!(
        fs::read(root.path().join("result.txt")).unwrap(),
        b"hello world"
    );
    assert_eq!(
        service.start_upload("/result.txt", false).unwrap_err().kind,
        FileErrorKind::AlreadyExists
    );

    let mut upload = service.start_upload("/result.txt", true).unwrap();
    upload.write_chunk(b"replaced").unwrap();
    upload.finish().unwrap();
    assert_eq!(
        fs::read(root.path().join("result.txt")).unwrap(),
        b"replaced"
    );

    let mut aborted = service.start_upload("/aborted.txt", false).unwrap();
    aborted.write_chunk(b"partial").unwrap();
    drop(aborted);
    assert!(!root.path().join("aborted.txt").exists());
    let leftovers: Vec<_> = fs::read_dir(root.path())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".anth-upload-")
        })
        .collect();
    assert!(leftovers.is_empty());
}

#[test]
fn enforces_the_upload_limit() {
    let root = TempRoot::new("upload-limit");
    let service = FileService::new(&FilesConfig {
        root: root.path().to_string_lossy().into_owned(),
        write: true,
        max_entries: 10,
        max_preview_bytes: 10,
        max_upload_bytes: 4,
        ..FilesConfig::default()
    })
    .unwrap();
    let mut upload = service.start_upload("/big", false).unwrap();

    assert_eq!(
        upload.write_chunk(b"12345").unwrap_err().kind,
        FileErrorKind::TooLarge
    );
    assert_eq!(upload.write_chunk(b"1234").unwrap(), 4);
}

#[test]
fn symlinks_are_removed_without_touching_the_target() {
    let root = TempRoot::new("delete-link");
    let outside = TempRoot::new("delete-target");
    write_file(outside.path(), "target", b"keep");
    let link = root.path().join("link");
    std::os::unix::fs::symlink(outside.path().join("target"), &link).unwrap();
    let service = service(root.path(), true);

    assert_eq!(service.delete("/link", false).unwrap(), "/link");
    assert!(fs::symlink_metadata(&link).is_err());
    assert_eq!(
        fs::read(outside.path().join("target")).unwrap(),
        b"keep".to_vec()
    );
}
