//! Archive guards and round trips, using the v0.1 criteria.
//!
//! Hostile members are assembled with the `tar` and `zip` writers instead of
//! the daemon's own writer, so a bug in `create` cannot make a guard look
//! correct. Every refusal also asserts on the filesystem: an error is not
//! enough if the file escaped anyway.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anth_panel::files::archive::{self, ArchiveLimits};
use anth_wire::ArchiveFormat;

struct Fixture {
    base: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = std::env::temp_dir().join(format!(
            "anth-archive-{name}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(base.join("src")).unwrap();
        fs::create_dir_all(base.join("out")).unwrap();
        Self { base }
    }

    fn src(&self) -> PathBuf {
        self.base.join("src")
    }

    fn out(&self) -> PathBuf {
        self.base.join("out")
    }

    fn write(&self, name: &str, body: &[u8]) -> PathBuf {
        let path = self.src().join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, body).unwrap();
        path
    }

    fn archive(&self, name: &str) -> PathBuf {
        self.base.join(name)
    }

    fn hostile_tar(&self, name: &str, members: &[(&str, &[u8])]) -> PathBuf {
        let path = self.archive(name);
        let file = fs::File::create(&path).unwrap();
        let mut builder = tar::Builder::new(file);
        for (member, body) in members {
            let mut header = tar::Header::new_gnu();
            let bytes = member.as_bytes();
            assert!(bytes.len() < 100);
            header.as_gnu_mut().unwrap().name[..bytes.len()].copy_from_slice(bytes);
            header.set_size(body.len() as u64);
            header.set_mode(0o644);
            header.set_mtime(0);
            header.set_cksum();
            builder.append(&header, *body).unwrap();
        }
        builder.finish().unwrap();
        path
    }

    fn hostile_zip(&self, name: &str, members: &[(&str, &[u8])]) -> PathBuf {
        let path = self.archive(name);
        let file = fs::File::create(&path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (member, body) in members {
            writer.start_file(*member, options).unwrap();
            writer.write_all(body).unwrap();
        }
        writer.finish().unwrap();
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

fn limits() -> ArchiveLimits {
    ArchiveLimits::default()
}

#[test]
fn safe_members_reject_every_escape_shape() {
    for name in [
        "",
        "/etc/passwd",
        "../x",
        "a/../../x",
        "a\\..\\x",
        "C:/x",
        "C:\\x",
        "a\0b",
    ] {
        assert!(
            archive::safe_member(name).is_none(),
            "`{name}` must be refused"
        );
    }
    assert_eq!(
        archive::safe_member("./docs/a.txt"),
        Some(PathBuf::from("docs/a.txt"))
    );
    assert_eq!(
        archive::safe_member("docs//a.txt"),
        Some(PathBuf::from("docs/a.txt"))
    );
}

#[test]
fn detects_archives_by_name_and_magic() {
    assert_eq!(
        archive::detect(Path::new("a.zip"), None),
        Some(ArchiveFormat::Zip)
    );
    assert_eq!(
        archive::detect(Path::new("a.tgz"), None),
        Some(ArchiveFormat::TarGz)
    );
    assert_eq!(
        archive::detect(Path::new("a.tar.xz"), None),
        Some(ArchiveFormat::TarXz)
    );
    assert_eq!(
        archive::detect(Path::new("a.7z"), None),
        Some(ArchiveFormat::SevenZ)
    );
    assert_eq!(
        archive::detect(Path::new("noext"), Some(b"PK\x03\x04")),
        Some(ArchiveFormat::Zip)
    );
    assert_eq!(
        archive::detect(Path::new("noext"), Some(b"\x1f\x8b")),
        Some(ArchiveFormat::TarGz)
    );
    assert_eq!(archive::detect(Path::new("a.txt"), Some(b"plain")), None);
}

#[test]
fn tar_gz_and_zip_round_trip_a_tree() {
    let fixture = Fixture::new("roundtrip");
    let source = fixture.write("project/a.txt", b"alpha");
    fs::create_dir_all(fixture.src().join("project/sub")).unwrap();
    fs::write(fixture.src().join("project/sub/b.txt"), b"beta").unwrap();
    let project = source.parent().unwrap();

    for (name, format) in [
        ("project.tar.gz", ArchiveFormat::TarGz),
        ("project.zip", ArchiveFormat::Zip),
        ("project.tar", ArchiveFormat::Tar),
    ] {
        let path = fixture.archive(name);
        archive::create(project, &path, format).unwrap();
        let out = fixture.out().join(name.replace('.', "-"));
        archive::extract(&path, format, &out, limits()).unwrap();
        assert_eq!(fs::read(out.join("project/a.txt")).unwrap(), b"alpha");
        assert_eq!(fs::read(out.join("project/sub/b.txt")).unwrap(), b"beta");
    }
}

#[test]
fn tar_xz_round_trips_a_tree() {
    let fixture = Fixture::new("xz");
    let source = fixture.write("project/a.txt", b"alpha");
    let project = source.parent().unwrap();
    let path = fixture.archive("project.tar.xz");

    archive::create(project, &path, ArchiveFormat::TarXz).unwrap();
    let out = fixture.out().join("xz");
    archive::extract(&path, ArchiveFormat::TarXz, &out, limits()).unwrap();

    assert_eq!(fs::read(out.join("project/a.txt")).unwrap(), b"alpha");
}

#[test]
fn a_tar_member_with_parent_components_is_refused_without_writing() {
    let fixture = Fixture::new("tar-dotdot");
    let path = fixture.hostile_tar("evil.tar", &[("../escape.txt", b"evil")]);
    let out = fixture.out().join("evil");

    let error = archive::extract(&path, ArchiveFormat::Tar, &out, limits());

    assert!(error.is_err());
    assert!(!fixture.base.join("escape.txt").exists());
    assert!(!out.join("escape.txt").exists());
}

#[test]
fn a_zip_member_with_parent_components_is_refused_without_writing() {
    let fixture = Fixture::new("zip-dotdot");
    let path = fixture.hostile_zip("evil.zip", &[("../escape.txt", b"evil")]);
    let out = fixture.out().join("evil");

    let error = archive::extract(&path, ArchiveFormat::Zip, &out, limits());

    assert!(error.is_err());
    assert!(!fixture.base.join("escape.txt").exists());
    assert!(!out.join("escape.txt").exists());
}

#[test]
fn a_symlink_followed_by_a_member_is_refused() {
    let fixture = Fixture::new("tar-symlink");
    let outside = fixture.base.join("outside");
    fs::create_dir_all(&outside).unwrap();
    let path = fixture.archive("evil.tar");
    let file = fs::File::create(&path).unwrap();
    let mut builder = tar::Builder::new(file);

    let mut link = tar::Header::new_gnu();
    link.set_entry_type(tar::EntryType::Symlink);
    link.set_size(0);
    link.set_mode(0o777);
    link.set_mtime(0);
    link.set_link_name(&outside).unwrap();
    let name = b"link";
    link.as_gnu_mut().unwrap().name[..name.len()].copy_from_slice(name);
    link.set_cksum();
    builder.append(&link, std::io::empty()).unwrap();

    let mut child = tar::Header::new_gnu();
    let child_name = b"link/owned.txt";
    child.as_gnu_mut().unwrap().name[..child_name.len()].copy_from_slice(child_name);
    child.set_size(4);
    child.set_mode(0o644);
    child.set_mtime(0);
    child.set_cksum();
    builder.append(&child, &b"evil"[..]).unwrap();
    builder.finish().unwrap();

    let out = fixture.out().join("evil");
    let error = archive::extract(&path, ArchiveFormat::Tar, &out, limits());

    assert!(error.is_err());
    assert!(!outside.join("owned.txt").exists());
}

#[test]
fn extraction_stops_past_the_total_limit() {
    let fixture = Fixture::new("limit");
    fixture.write("big/a.bin", &[b'x'; 10_000]);
    let source = fixture.src().join("big");
    let path = fixture.archive("big.tar");
    archive::create(&source, &path, ArchiveFormat::Tar).unwrap();

    let out = fixture.out().join("big");
    let error = archive::extract(
        &path,
        ArchiveFormat::Tar,
        &out,
        ArchiveLimits {
            max_total_bytes: 100,
            max_entries: 100,
            max_entry_bytes: 100,
        },
    );

    assert!(error.is_err());
    // The extractor stops mid-entry; a partial file may remain, but it must
    // never exceed the budget and the request must fail.
    let partial = out.join("big/a.bin");
    assert!(
        fs::metadata(&partial)
            .map(|m| m.len() <= 100)
            .unwrap_or(true)
    );
}

#[test]
fn a_single_wrapper_directory_is_unwrapped_when_the_destination_matches() {
    let fixture = Fixture::new("unwrap");
    fixture.write("docs/a.txt", b"a");
    let source = fixture.src().join("docs");
    let path = fixture.archive("docs.zip");
    archive::create(&source, &path, ArchiveFormat::Zip).unwrap();

    let out = fixture.out().join("docs");
    fs::create_dir_all(&out).unwrap();
    archive::extract_with(&path, ArchiveFormat::Zip, &out, limits(), Some("docs")).unwrap();

    assert_eq!(fs::read(out.join("a.txt")).unwrap(), b"a");
    assert!(!out.join("docs/a.txt").exists());
}
