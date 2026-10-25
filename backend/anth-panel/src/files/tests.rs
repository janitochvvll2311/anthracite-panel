//! `file read`, over the real parser and a real fixture tree.
//!
//! Every fixture writes a file under `temp_dir()` and panics if it cannot, so a
//! missing file fails the setup rather than making an assertion trivially true.

use std::path::{Path, PathBuf};

use anth_wire::{ErrorKind, Request, Response};

use crate::dispatch::Daemon;
use crate::plugins::registry::Registry;

/// A fresh directory, removed by the caller.
fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("anth-files-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("a fixture directory must be creatable");
    root
}

fn write(root: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = root.join(name);
    std::fs::write(&path, bytes).expect("a fixture file must be writable");
    path
}

fn daemon(root: &Path) -> Daemon {
    Daemon::new(Registry::empty(), crate::tests::quiet(), root)
}

async fn read(root: &Path, path: &str, offset: u64, len: u32) -> Response {
    daemon(root)
        .handle(Request::FileRead {
            path: path.to_string(),
            offset,
            len,
        })
        .await
}

async fn slice(root: &Path, path: &str, offset: u64, len: u32) -> (Vec<u8>, u64, bool) {
    match read(root, path, offset, len).await {
        Response::FileSlice {
            bytes, next, eof, ..
        } => (bytes, next, eof),
        other => panic!("expected a slice, got {other:?}"),
    }
}

#[tokio::test]
async fn a_whole_small_file_arrives_in_one_slice_marked_at_the_end() {
    let root = scratch("small");
    write(&root, "hello.txt", b"hello\n");

    let (bytes, next, eof) = slice(&root, "hello.txt", 0, 1024).await;

    assert_eq!(bytes, b"hello\n");
    assert_eq!(next, 6);
    assert!(eof, "a slice that reached the end must say so");
    std::fs::remove_dir_all(&root).expect("the fixture must be removable");
}

/// The point of `eof`: an empty answer before the end and an empty answer at the
/// end are different, and only the flag tells them apart.
#[tokio::test]
async fn reading_past_the_end_is_an_empty_slice_not_an_error() {
    let root = scratch("past-end");
    write(&root, "five.txt", b"12345");

    let (bytes, next, eof) = slice(&root, "five.txt", 99, 16).await;

    assert!(bytes.is_empty());
    assert_eq!(
        next, 5,
        "the cursor stays at the end, not at the offset asked"
    );
    assert!(eof);
    std::fs::remove_dir_all(&root).expect("the fixture must be removable");
}

/// A file larger than one slice is read piece by piece, and the pieces
/// concatenate to the original bytes — the property a copy depends on.
#[tokio::test]
async fn a_file_larger_than_a_slice_is_read_in_pieces_that_join_up() {
    let root = scratch("pieces");
    let content: Vec<u8> = (0..(3 * crate::files::MAX_SLICE + 7))
        .map(|i| (i % 251) as u8)
        .collect();
    write(&root, "big.bin", &content);

    let mut collected = Vec::new();
    let mut offset = 0;
    let step = crate::files::MAX_SLICE;
    loop {
        let (bytes, next, eof) = slice(&root, "big.bin", offset, step).await;
        collected.extend_from_slice(&bytes);
        if eof {
            break;
        }
        assert!(next > offset, "a slice that is not the end must advance");
        offset = next;
    }

    assert_eq!(collected, content, "the pieces must equal the file");
    std::fs::remove_dir_all(&root).expect("the fixture must be removable");
}

/// No size-fitting: a caller asking for more than the cap is refused, never
/// handed a short answer that looks complete.
#[tokio::test]
async fn asking_for_more_than_the_cap_is_refused_rather_than_shortened() {
    let root = scratch("over-cap");
    write(&root, "five.txt", b"12345");

    match read(&root, "five.txt", 0, crate::files::MAX_SLICE + 1).await {
        Response::Error(error) => {
            assert_eq!(error.kind, ErrorKind::Invalid);
            assert!(
                error.message.contains("exceeds"),
                "the message must name the limit: {}",
                error.message
            );
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
    std::fs::remove_dir_all(&root).expect("the fixture must be removable");
}

#[tokio::test]
async fn a_zero_length_read_is_refused() {
    let root = scratch("zero");
    write(&root, "five.txt", b"12345");

    match read(&root, "five.txt", 0, 0).await {
        Response::Error(error) => assert_eq!(error.kind, ErrorKind::Invalid),
        other => panic!("expected a refusal, got {other:?}"),
    }
    std::fs::remove_dir_all(&root).expect("the fixture must be removable");
}

/// The boundary is the resolved path, so `..` is refused even when it lands
/// back inside the root: the message should name what the caller typed.
#[tokio::test]
async fn a_parent_component_is_refused_by_name() {
    let root = scratch("parent");
    write(&root, "five.txt", b"12345");

    match read(&root, "sub/../five.txt", 0, 16).await {
        Response::Error(error) => {
            assert_eq!(error.kind, ErrorKind::Forbidden);
            assert!(error.message.contains(".."), "{}", error.message);
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
    std::fs::remove_dir_all(&root).expect("the fixture must be removable");
}

#[tokio::test]
async fn an_absolute_path_is_refused() {
    let root = scratch("absolute");
    write(&root, "five.txt", b"12345");
    let absolute = root.join("five.txt");

    match read(
        &root,
        absolute.to_str().expect("a utf-8 fixture path"),
        0,
        16,
    )
    .await
    {
        Response::Error(error) => {
            assert_eq!(error.kind, ErrorKind::Forbidden);
            assert!(error.message.contains("absolute"), "{}", error.message);
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
    std::fs::remove_dir_all(&root).expect("the fixture must be removable");
}

/// A symlink inside the root that points out of it must not be followed. This
/// is the escape a lexical prefix check would miss.
#[cfg(unix)]
#[tokio::test]
async fn a_symlink_that_escapes_the_root_is_refused() {
    let root = scratch("symlink");
    let outside = scratch("symlink-outside");
    write(&outside, "secret.txt", b"not yours");
    std::os::unix::fs::symlink(outside.join("secret.txt"), root.join("escape.txt"))
        .expect("a fixture symlink must be creatable");

    match read(&root, "escape.txt", 0, 16).await {
        Response::Error(error) => {
            assert_eq!(error.kind, ErrorKind::Forbidden);
            assert!(
                error.message.contains("outside"),
                "the message must say why: {}",
                error.message
            );
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
    std::fs::remove_dir_all(&root).expect("the fixture must be removable");
    std::fs::remove_dir_all(&outside).expect("the fixture must be removable");
}

/// A symlink that stays inside the root is followed: the rule is escape, not
/// the presence of a link.
#[cfg(unix)]
#[tokio::test]
async fn a_symlink_inside_the_root_is_followed() {
    let root = scratch("symlink-inside");
    write(&root, "real.txt", b"inside");
    std::os::unix::fs::symlink(root.join("real.txt"), root.join("link.txt"))
        .expect("a fixture symlink must be creatable");

    let (bytes, _, eof) = slice(&root, "link.txt", 0, 16).await;

    assert_eq!(bytes, b"inside");
    assert!(eof);
    std::fs::remove_dir_all(&root).expect("the fixture must be removable");
}

#[tokio::test]
async fn a_missing_file_is_a_404_naming_it() {
    let root = scratch("missing");

    match read(&root, "nope.txt", 0, 16).await {
        Response::Error(error) => {
            assert_eq!(error.kind, ErrorKind::NotFound);
            assert!(error.message.contains("nope.txt"), "{}", error.message);
        }
        other => panic!("expected a miss, got {other:?}"),
    }
    std::fs::remove_dir_all(&root).expect("the fixture must be removable");
}

/// Resuming is the reason `offset` exists: a client that stopped mid-file asks
/// again from the cursor and gets the rest, not the whole file.
#[tokio::test]
async fn resuming_from_an_offset_returns_only_the_rest() {
    let root = scratch("resume");
    write(&root, "abcdef.txt", b"abcdef");

    let (bytes, next, eof) = slice(&root, "abcdef.txt", 2, 3).await;

    assert_eq!(bytes, b"cde");
    assert_eq!(next, 5);
    assert!(!eof);

    let (rest, _, eof) = slice(&root, "abcdef.txt", next, 16).await;
    assert_eq!(rest, b"f");
    assert!(eof);
    std::fs::remove_dir_all(&root).expect("the fixture must be removable");
}

/// A path that reports size zero and still has content must be read, not
/// answered with an empty `eof`. `/proc/loadavg` is the everyday case, and the
/// bug this pins is a read that returns nothing while answering "end of file".
#[tokio::test]
async fn a_path_that_reports_size_zero_is_still_read() {
    let root = std::path::Path::new("/proc");
    if !root.join("loadavg").exists() {
        // No `/proc` means no such pseudo-file; the property is about procfs.
        return;
    }

    let (bytes, next, eof) = slice(root, "loadavg", 0, 4096).await;

    assert!(!bytes.is_empty(), "a size of zero is not an empty file");
    assert_eq!(next, bytes.len() as u64);
    assert!(eof, "a short read is the only trustworthy end");
}

/// The other half: a file that really is empty answers `eof` with nothing in it.
#[tokio::test]
async fn a_really_empty_file_is_an_empty_slice_at_the_end() {
    let root = scratch("empty");
    write(&root, "nothing.txt", b"");

    let (bytes, next, eof) = slice(&root, "nothing.txt", 0, 16).await;

    assert!(bytes.is_empty());
    assert_eq!(next, 0);
    assert!(eof);
    std::fs::remove_dir_all(&root).expect("the fixture must be removable");
}

/// A FIFO can block a read forever, and `offset` cannot be honoured on
/// something that cannot be seeked. One request must stay bounded, so the shape
/// is refused rather than waited on.
#[cfg(unix)]
#[tokio::test]
async fn a_fifo_is_refused_instead_of_waited_on() {
    let root = scratch("fifo");
    let fifo = root.join("pipe");
    let made = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .expect("mkfifo must be runnable");
    assert!(made.success(), "the fixture FIFO must be creatable");

    match read(&root, "pipe", 0, 16).await {
        Response::Error(error) => {
            assert_eq!(error.kind, ErrorKind::Forbidden);
            assert!(
                error.message.contains("regular file"),
                "the message must name the rule: {}",
                error.message
            );
        }
        other => panic!("expected a refusal, got {other:?}"),
    }

    std::fs::remove_dir_all(&root).expect("the fixture must be removable");
}
