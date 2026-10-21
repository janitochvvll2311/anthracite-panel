//! Archive tests, with the hostile cases written first.
//!
//! The archives are built by `tar` and `python3`'s `zipfile` rather than by this
//! crate's own writer, so a bug in the writer cannot make a broken guard look
//! correct. Each hostile archive asserts on the filesystem as well as the error:
//! "it returned an error" is not enough if the file escaped anyway.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anth_panel::files::archive::{self, ArchiveLimits};
use anth_proto::{ArchiveFormat, FileErrorKind, FileKind};

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

    /// Builds a tar with an arbitrary member name, which is how the hostile
    /// cases get a `..` into the archive: GNU tar refuses to create one, so the
    /// header is assembled through Python's `tarfile`.
    fn hostile_tar(&self, name: &str, members: &[(&str, &str)]) -> PathBuf {
        let path = self.archive(name);
        let script = format!(
            r#"
import tarfile, io, sys
with tarfile.open({path:?}, "w:gz") as tar:
    for member, body in {members:?}:
        data = body.encode()
        info = tarfile.TarInfo(member)
        info.size = len(data)
        tar.addfile(info, io.BytesIO(data))
"#,
            path = path.to_string_lossy(),
            members = members,
        );
        let status = Command::new("python3")
            .arg("-c")
            .arg(script)
            .status()
            .expect("python3 is required to build the hostile archives");
        assert!(status.success(), "python3 could not write {name}");
        path
    }

    /// Builds a zip with an arbitrary member name.
    fn hostile_zip(&self, name: &str, members: &[(&str, &str)]) -> PathBuf {
        let path = self.archive(name);
        let script = format!(
            r#"
import zipfile
with zipfile.ZipFile({path:?}, "w") as zf:
    for member, body in {members:?}:
        zf.writestr(member, body)
"#,
            path = path.to_string_lossy(),
            members = members,
        );
        let status = Command::new("python3")
            .arg("-c")
            .arg(script)
            .status()
            .expect("python3 is required to build the hostile archives");
        assert!(status.success(), "python3 could not write {name}");
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
fn a_tar_gz_round_trips_a_directory_tree() {
    let fixture = Fixture::new("roundtrip");
    fixture.write("project/a.txt", b"alpha");
    fixture.write("project/sub/b.txt", b"beta");

    let archive_path = fixture.archive("project.tar.gz");
    archive::create(
        &fixture.src().join("project"),
        &archive_path,
        ArchiveFormat::TarGz,
    )
    .unwrap();

    let out = fixture.out().join("extracted");
    fs::create_dir_all(&out).unwrap();
    let written = archive::extract(&archive_path, ArchiveFormat::TarGz, &out, limits()).unwrap();

    assert_eq!(
        fs::read(out.join("project/a.txt")).unwrap(),
        b"alpha",
        "written: {written:?}"
    );
    assert_eq!(fs::read(out.join("project/sub/b.txt")).unwrap(), b"beta");
}

#[test]
fn zip_and_xz_round_trip() {
    let fixture = Fixture::new("formats");
    fixture.write("data/one.txt", b"1");
    fixture.write("data/two.txt", b"2");
    let source = fixture.src().join("data");

    for (name, format) in [
        ("data.zip", ArchiveFormat::Zip),
        ("data.tar.xz", ArchiveFormat::TarXz),
        ("data.tar", ArchiveFormat::Tar),
    ] {
        let archive_path = fixture.archive(name);
        archive::create(&source, &archive_path, format).unwrap();

        let out = fixture.out().join(name);
        fs::create_dir_all(&out).unwrap();
        archive::extract(&archive_path, format, &out, limits()).unwrap();

        assert_eq!(
            fs::read(out.join("data/one.txt")).unwrap(),
            b"1",
            "{name} lost one.txt"
        );
        assert_eq!(
            fs::read(out.join("data/two.txt")).unwrap(),
            b"2",
            "{name} lost two.txt"
        );
    }
}

#[test]
fn created_archives_are_readable_by_the_system_tools() {
    // The point of using these crates is interoperability, so the check is an
    // independent implementation, not our own reader.
    let fixture = Fixture::new("interop");
    fixture.write("dir/file.txt", b"interop");
    let source = fixture.src().join("dir");

    for (name, tool) in [
        ("dir.tar.gz", vec!["tar", "tzf"]),
        ("dir.tar.xz", vec!["tar", "tJf"]),
        ("dir.tar", vec!["tar", "tf"]),
    ] {
        let archive_path = fixture.archive(name);
        let format = if name.ends_with(".gz") {
            ArchiveFormat::TarGz
        } else if name.ends_with(".xz") {
            ArchiveFormat::TarXz
        } else {
            ArchiveFormat::Tar
        };
        archive::create(&source, &archive_path, format).unwrap();

        let output = Command::new(tool[0])
            .arg(tool[1])
            .arg(&archive_path)
            .output()
            .expect("tar is required for the interop check");
        assert!(
            output.status.success(),
            "{name} was not readable by tar: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let listing = String::from_utf8_lossy(&output.stdout);
        assert!(
            listing.contains("dir/file.txt"),
            "{name} listing was {listing}"
        );
    }

    // Zip through Python's zipfile, which is the reference implementation most
    // users' tools agree with.
    let archive_path = fixture.archive("dir.zip");
    archive::create(&source, &archive_path, ArchiveFormat::Zip).unwrap();
    let status = Command::new("python3")
        .arg("-c")
        .arg(format!(
            "import zipfile,sys; z=zipfile.ZipFile({:?}); sys.exit(0 if 'dir/file.txt' in z.namelist() else 1)",
            archive_path.to_string_lossy()
        ))
        .status()
        .unwrap();
    assert!(status.success(), "python3 could not read the zip we wrote");
}

#[test]
fn listing_reports_the_members_without_extracting() {
    let fixture = Fixture::new("listing");
    fixture.write("pair/a.txt", b"aaaa");
    fixture.write("pair/b.txt", b"bb");

    for (name, format) in [
        ("pair.zip", ArchiveFormat::Zip),
        ("pair.tar.gz", ArchiveFormat::TarGz),
        ("pair.tar.xz", ArchiveFormat::TarXz),
    ] {
        let archive_path = fixture.archive(name);
        archive::create(&fixture.src().join("pair"), &archive_path, format).unwrap();

        let entries = archive::list(&archive_path, format, 100).unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();

        assert!(
            names.contains(&"pair/a.txt"),
            "{name} listing was {names:?}"
        );
        let file = entries
            .iter()
            .find(|e| e.name == "pair/a.txt")
            .expect("a.txt must be listed");
        assert_eq!(file.size, 4);
        assert_eq!(file.kind, FileKind::File);
    }
}

#[test]
fn listing_respects_the_entry_cap() {
    let fixture = Fixture::new("list-cap");
    for index in 0..5 {
        fixture.write(&format!("many/f{index}.txt"), b"x");
    }
    let archive_path = fixture.archive("many.zip");
    archive::create(
        &fixture.src().join("many"),
        &archive_path,
        ArchiveFormat::Zip,
    )
    .unwrap();

    let entries = archive::list(&archive_path, ArchiveFormat::Zip, 2).unwrap();

    assert_eq!(entries.len(), 2);
}

#[test]
fn a_tar_with_a_parent_traversal_member_is_refused() {
    let fixture = Fixture::new("tar-traversal");
    let archive_path = fixture.hostile_tar(
        "evil.tar.gz",
        &[("../escaped.txt", "pwned"), ("ok.txt", "fine")],
    );

    let error = archive::extract(
        &archive_path,
        ArchiveFormat::TarGz,
        &fixture.out(),
        limits(),
    )
    .unwrap_err();

    assert_eq!(error.kind, FileErrorKind::InvalidPath);
    assert!(
        !fixture.base.join("escaped.txt").exists(),
        "the entry escaped the destination"
    );
}

#[test]
fn a_tar_with_an_absolute_member_is_refused() {
    let fixture = Fixture::new("tar-absolute");
    let marker = fixture.base.join("absolute-marker.txt");
    let member = marker.to_string_lossy().into_owned();
    let archive_path = fixture.hostile_tar("abs.tar.gz", &[(member.as_str(), "pwned")]);

    let error = archive::extract(
        &archive_path,
        ArchiveFormat::TarGz,
        &fixture.out(),
        limits(),
    )
    .unwrap_err();

    assert_eq!(error.kind, FileErrorKind::InvalidPath);
    assert!(!marker.exists(), "an absolute member was written");
}

#[test]
fn a_zip_with_a_parent_traversal_member_is_refused() {
    let fixture = Fixture::new("zip-traversal");
    let archive_path = fixture.hostile_zip(
        "evil.zip",
        &[
            ("../../escaped.txt", "pwned"),
            ("nested/../../escaped2.txt", "pwned"),
        ],
    );

    let error =
        archive::extract(&archive_path, ArchiveFormat::Zip, &fixture.out(), limits()).unwrap_err();

    assert_eq!(error.kind, FileErrorKind::InvalidPath);
    assert!(!fixture.base.join("escaped.txt").exists());
    assert!(!fixture.base.join("escaped2.txt").exists());
}

#[test]
fn a_backslash_member_is_refused() {
    // A backslash is not a separator on Unix, so a naive check would accept
    // this and write a file literally named `..\..\x` — which becomes an escape
    // the moment the tree is copied to Windows.
    let fixture = Fixture::new("backslash");
    let archive_path = fixture.hostile_zip("win.zip", &[("..\\..\\escaped.txt", "pwned")]);

    let error =
        archive::extract(&archive_path, ArchiveFormat::Zip, &fixture.out(), limits()).unwrap_err();

    assert_eq!(error.kind, FileErrorKind::InvalidPath);
}

#[test]
fn a_symlink_that_a_later_member_writes_through_is_refused() {
    // The classic two-step escape: store `link -> /tmp`, then store
    // `link/owned.txt`. Following the link would write outside the destination.
    let fixture = Fixture::new("symlink");
    let outside = fixture.base.join("outside");
    fs::create_dir_all(&outside).unwrap();
    let archive_path = fixture.base.join("link.tar");
    let script = format!(
        r#"
import tarfile, io
with tarfile.open({path:?}, "w") as tar:
    info = tarfile.TarInfo("link")
    info.type = tarfile.SYMTYPE
    info.linkname = {outside:?}
    tar.addfile(info)
    data = b"pwned"
    body = tarfile.TarInfo("link/owned.txt")
    body.size = len(data)
    tar.addfile(body, io.BytesIO(data))
"#,
        path = archive_path.to_string_lossy(),
        outside = outside.to_string_lossy(),
    );
    let status = Command::new("python3")
        .arg("-c")
        .arg(script)
        .status()
        .unwrap();
    assert!(
        status.success(),
        "python3 could not write the symlink archive"
    );

    let result = archive::extract(&archive_path, ArchiveFormat::Tar, &fixture.out(), limits());

    assert!(
        !outside.join("owned.txt").exists(),
        "the symlink was followed and wrote outside the destination"
    );
    // Either the archive is refused or the entry is skipped; what must not
    // happen is a write through the link.
    if let Err(error) = result {
        assert!(
            matches!(error.kind, FileErrorKind::InvalidPath | FileErrorKind::Io),
            "unexpected error kind {:?}",
            error.kind
        );
    }
}

#[test]
fn a_zip_bomb_is_stopped_by_the_total_limit() {
    let fixture = Fixture::new("bomb");
    let archive_path = fixture.base.join("bomb.zip");
    let script = format!(
        r#"
import zipfile
with zipfile.ZipFile({path:?}, "w", zipfile.ZIP_DEFLATED) as zf:
    zf.writestr("big.bin", b"\0" * (8 * 1024 * 1024))
"#,
        path = archive_path.to_string_lossy(),
    );
    assert!(
        Command::new("python3")
            .arg("-c")
            .arg(script)
            .status()
            .unwrap()
            .success()
    );

    let error = archive::extract(
        &archive_path,
        ArchiveFormat::Zip,
        &fixture.out(),
        ArchiveLimits {
            max_total_bytes: 1024 * 1024,
            max_entries: 100,
            max_entry_bytes: 1024 * 1024,
        },
    )
    .unwrap_err();

    assert_eq!(error.kind, FileErrorKind::TooLarge);
    let written = fs::metadata(fixture.out().join("big.bin"))
        .map(|m| m.len())
        .unwrap_or(0);
    assert!(
        written <= 1024 * 1024,
        "the extractor kept writing past the limit: {written} bytes"
    );
}

#[test]
fn an_archive_with_too_many_entries_is_refused() {
    let fixture = Fixture::new("entry-cap");
    let archive_path = fixture.base.join("many.zip");
    let script = format!(
        r#"
import zipfile
with zipfile.ZipFile({path:?}, "w") as zf:
    for i in range(20):
        zf.writestr(f"f{{i}}.txt", b"x")
"#,
        path = archive_path.to_string_lossy(),
    );
    assert!(
        Command::new("python3")
            .arg("-c")
            .arg(script)
            .status()
            .unwrap()
            .success()
    );

    let error = archive::extract(
        &archive_path,
        ArchiveFormat::Zip,
        &fixture.out(),
        ArchiveLimits {
            max_total_bytes: 1024 * 1024,
            max_entries: 5,
            max_entry_bytes: 1024 * 1024,
        },
    )
    .unwrap_err();

    assert_eq!(error.kind, FileErrorKind::TooLarge);
}

#[test]
fn creating_a_7z_is_refused_with_a_useful_message() {
    let fixture = Fixture::new("sevenz-create");
    fixture.write("a.txt", b"a");

    let error = archive::create(
        &fixture.src().join("a.txt"),
        &fixture.archive("a.7z"),
        ArchiveFormat::SevenZ,
    )
    .unwrap_err();

    assert_eq!(error.kind, FileErrorKind::Disabled);
    assert!(
        error.detail.contains("not supported"),
        "the message should say what is missing, got `{}`",
        error.detail
    );
}

#[test]
fn a_7z_created_by_another_tool_is_extracted() {
    // Our own writer cannot make a 7z, so the fixture comes from this crate's
    // compressor used directly — the point is that the reader works, and the
    // format is externally produced by definition.
    let fixture = Fixture::new("sevenz-read");
    let source = fixture.write("single.txt", b"sevenz-body");

    let archive_path = fixture.archive("single.7z");
    sevenz_rust2::compress_to_path(&source, &archive_path).unwrap();

    let out = fixture.out().join("sevenz");
    fs::create_dir_all(&out).unwrap();
    archive::extract(&archive_path, ArchiveFormat::SevenZ, &out, limits()).unwrap();

    assert_eq!(fs::read(out.join("single.txt")).unwrap(), b"sevenz-body");
}

#[test]
fn detect_falls_back_to_magic_for_a_renamed_file() {
    let fixture = Fixture::new("detect");
    fixture.write("a.txt", b"a");
    let archive_path = fixture.archive("a.tar.gz");
    archive::create(
        &fixture.src().join("a.txt"),
        &archive_path,
        ArchiveFormat::TarGz,
    )
    .unwrap();

    let renamed = fixture.base.join("no-extension");
    fs::copy(&archive_path, &renamed).unwrap();
    let magic = fs::read(&renamed).unwrap();
    let magic = &magic[..magic.len().min(300)];

    assert_eq!(
        archive::detect(&renamed, Some(magic)),
        Some(ArchiveFormat::TarGz)
    );
}

#[test]
fn a_tar_with_a_dot_prefixed_ordinary_name_is_still_extracted() {
    // `./a.txt` is common in archives made by `tar -C dir .` and must not be
    // treated as an escape.
    let fixture = Fixture::new("dot-prefix");
    // The helper writes gzip, so the format has to match the bytes.
    let archive_path = fixture.hostile_tar("dot.tar.gz", &[("./a.txt", "content")]);

    let out = fixture.out().join("dot");
    fs::create_dir_all(&out).unwrap();
    archive::extract(&archive_path, ArchiveFormat::TarGz, &out, limits()).unwrap();

    assert_eq!(fs::read(out.join("a.txt")).unwrap(), b"content");
}

#[test]
fn extraction_writes_into_a_missing_destination_subtree() {
    let fixture = Fixture::new("nested");
    fixture.write("deep/a/b/c.txt", b"deep");
    let archive_path = fixture.archive("deep.zip");
    archive::create(
        &fixture.src().join("deep"),
        &archive_path,
        ArchiveFormat::Zip,
    )
    .unwrap();

    let out = fixture.out().join("fresh");
    fs::create_dir_all(&out).unwrap();
    archive::extract(&archive_path, ArchiveFormat::Zip, &out, limits()).unwrap();

    assert_eq!(fs::read(out.join("deep/a/b/c.txt")).unwrap(), b"deep");
}

#[test]
fn a_truncated_archive_reports_an_error_rather_than_a_partial_success() {
    let fixture = Fixture::new("truncated");
    fixture.write("a.txt", b"a".repeat(10_000).as_slice());
    let archive_path = fixture.archive("a.tar.gz");
    archive::create(
        &fixture.src().join("a.txt"),
        &archive_path,
        ArchiveFormat::TarGz,
    )
    .unwrap();

    // Cut the file in the middle, as a failed download would.
    let bytes = fs::read(&archive_path).unwrap();
    fs::write(&archive_path, &bytes[..bytes.len() / 2]).unwrap();

    let result = archive::extract(
        &archive_path,
        ArchiveFormat::TarGz,
        &fixture.out(),
        limits(),
    );

    assert!(
        result.is_err(),
        "a truncated archive must not report success"
    );
}

#[test]
fn extracting_into_a_destination_named_after_the_wrapper_does_not_nest_it() {
    // The end-to-end run produced `docs/docs/a.txt` because the destination was
    // named `docs` and the archive's wrapper directory was also `docs`.
    let fixture = Fixture::new("strip-root");
    fixture.write("docs/a.txt", b"alpha");
    let archive_path = fixture.archive("docs.zip");
    archive::create(
        &fixture.src().join("docs"),
        &archive_path,
        ArchiveFormat::Zip,
    )
    .unwrap();

    let out = fixture.out().join("docs");
    fs::create_dir_all(&out).unwrap();
    archive::extract_with(
        &archive_path,
        ArchiveFormat::Zip,
        &out,
        limits(),
        Some("docs"),
    )
    .unwrap();

    assert_eq!(fs::read(out.join("a.txt")).unwrap(), b"alpha");
    assert!(
        !out.join("docs").exists(),
        "the wrapper directory was nested inside the destination"
    );
}

#[test]
fn an_unrelated_destination_keeps_the_wrapper_directory() {
    // Stripping must be conditional: extracting into `elsewhere` should keep
    // the archive's own layout rather than scattering its members.
    let fixture = Fixture::new("no-strip");
    fixture.write("docs/a.txt", b"alpha");
    let archive_path = fixture.archive("docs.zip");
    archive::create(
        &fixture.src().join("docs"),
        &archive_path,
        ArchiveFormat::Zip,
    )
    .unwrap();

    let out = fixture.out().join("elsewhere");
    fs::create_dir_all(&out).unwrap();
    archive::extract_with(
        &archive_path,
        ArchiveFormat::Zip,
        &out,
        limits(),
        Some("elsewhere"),
    )
    .unwrap();

    assert_eq!(fs::read(out.join("docs/a.txt")).unwrap(), b"alpha");
}

#[test]
fn an_archive_without_a_common_root_is_not_stripped() {
    let fixture = Fixture::new("mixed-root");
    let archive_path = fixture.hostile_zip("mixed.zip", &[("one/a.txt", "a"), ("two/b.txt", "b")]);

    let out = fixture.out().join("mixed");
    fs::create_dir_all(&out).unwrap();
    archive::extract_with(
        &archive_path,
        ArchiveFormat::Zip,
        &out,
        limits(),
        Some("one"),
    )
    .unwrap();

    assert!(out.join("one/a.txt").exists());
    assert!(out.join("two/b.txt").exists());
}

#[test]
fn detects_every_supported_extension() {
    let fixtures: &[(&str, ArchiveFormat)] = &[
        ("a.zip", ArchiveFormat::Zip),
        ("a.tar", ArchiveFormat::Tar),
        ("a.tar.gz", ArchiveFormat::TarGz),
        ("a.tgz", ArchiveFormat::TarGz),
        ("a.tar.xz", ArchiveFormat::TarXz),
        ("a.txz", ArchiveFormat::TarXz),
        ("a.7z", ArchiveFormat::SevenZ),
    ];
    for (name, expected) in fixtures {
        assert_eq!(
            archive::detect(Path::new(name), None),
            Some(*expected),
            "{name} was misdetected"
        );
    }
    assert_eq!(archive::detect(Path::new("a.rar"), None), None);
}

#[test]
fn a_final_component_symlink_cannot_redirect_a_write_outside() {
    // A lone symlink member used to be enough: prepare the parent, then let
    // File::create follow the link the archive just wrote. The final component
    // must be refused before anything is opened.
    let fixture = Fixture::new("final-symlink");
    let outside = fixture.base.join("outside");
    fs::create_dir_all(&outside).unwrap();
    let victim = outside.join("owned.txt");
    fs::write(&victim, "original").unwrap();
    let archive_path = fixture.base.join("final.tar");
    let script = format!(
        r#"
import tarfile
with tarfile.open({path:?}, "w") as tar:
    info = tarfile.TarInfo("owned.txt")
    info.type = tarfile.SYMTYPE
    info.linkname = {victim:?}
    tar.addfile(info)
"#,
        path = archive_path.to_string_lossy(),
        victim = victim.to_string_lossy(),
    );
    assert!(
        Command::new("python3")
            .arg("-c")
            .arg(script)
            .status()
            .unwrap()
            .success(),
        "python3 could not write the symlink archive"
    );

    let result = archive::extract(&archive_path, ArchiveFormat::Tar, &fixture.out(), limits());

    assert_eq!(
        fs::read_to_string(&victim).unwrap(),
        "original",
        "the final-component symlink was followed out of the destination"
    );
    if let Err(error) = result {
        assert!(
            matches!(error.kind, FileErrorKind::InvalidPath | FileErrorKind::Io),
            "unexpected error kind {:?}",
            error.kind
        );
    }
}

#[test]
fn a_tar_xz_bomb_is_stopped_without_materializing_it() {
    // `lzma_rs` can only decompress into a sink, so a naive implementation
    // inflates the whole stream in memory before the entry budget ever runs.
    let fixture = Fixture::new("xz-bomb");
    let archive_path = fixture.base.join("bomb.tar.xz");
    let script = format!(
        r#"
import tarfile, io, lzma
body = b"\0" * (64 * 1024 * 1024)
with lzma.open({path:?}, "wb", format=lzma.FORMAT_XZ, preset=9) as xz:
    with tarfile.open(fileobj=xz, mode="w") as tar:
        info = tarfile.TarInfo("big.bin")
        info.size = len(body)
        tar.addfile(info, io.BytesIO(body))
"#,
        path = archive_path.to_string_lossy(),
    );
    assert!(
        Command::new("python3")
            .arg("-c")
            .arg(script)
            .status()
            .unwrap()
            .success(),
        "python3 could not write the xz archive"
    );

    let error = archive::extract(
        &archive_path,
        ArchiveFormat::TarXz,
        &fixture.out(),
        ArchiveLimits {
            max_total_bytes: 1024 * 1024,
            max_entries: 100,
            max_entry_bytes: 1024 * 1024,
        },
    )
    .unwrap_err();

    assert_eq!(error.kind, FileErrorKind::TooLarge);
    let written = fs::metadata(fixture.out().join("big.bin"))
        .map(|m| m.len())
        .unwrap_or(0);
    assert!(
        written <= 1024 * 1024,
        "the extractor kept writing past the limit: {written} bytes"
    );
}
