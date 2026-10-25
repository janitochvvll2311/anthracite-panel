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
    read_waiting(root, path, offset, len, 0).await
}

async fn read_waiting(root: &Path, path: &str, offset: u64, len: u32, wait_ms: u32) -> Response {
    daemon(root)
        .handle(Request::FileRead {
            path: path.to_string(),
            offset,
            len,
            wait_ms,
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

/// A follower must be able to tell "the file ends here" from "nothing arrived".
///
/// Both answers are an empty slice at the same cursor, so only `waited` tells
/// them apart. Without it, a follower that caught up would decide the file was
/// finished and stop, which is the silent truncation this rewrite removes.
#[tokio::test]
async fn a_wait_that_finds_nothing_says_so_instead_of_looking_like_an_end() {
    let root = scratch("wait-empty");
    write(&root, "quiet.log", b"one line\n");

    // A copy: no wait, so the empty answer at the end is the end.
    let copied = read_waiting(&root, "quiet.log", 9, 64, 0).await;
    match copied {
        Response::FileSlice { eof, waited, .. } => {
            assert!(eof, "a copy at the end must say the file ended");
            assert!(!waited, "a copy must not claim to have waited");
        }
        other => panic!("expected a slice, got {other:?}"),
    }

    // A follower: it waits, finds nothing, and says that instead.
    let followed = read_waiting(&root, "quiet.log", 9, 64, 100).await;
    match followed {
        Response::FileSlice {
            eof,
            waited,
            next,
            bytes,
            ..
        } => {
            assert!(waited, "the daemon waited, so the answer must say it did");
            assert!(eof, "the file still has no more to give");
            assert!(bytes.is_empty(), "nothing arrived, so nothing is returned");
            assert_eq!(next, 9, "the cursor must not move when nothing arrived");
        }
        other => panic!("expected a slice, got {other:?}"),
    }

    std::fs::remove_dir_all(&root).expect("the fixture must be removable");
}

/// A wait returns as soon as the file grows, not when the wait expires.
///
/// The byte is appended from another thread after a delay that is well inside
/// the wait, so the assertion is about the daemon copying the file and not
/// about how long the test machine takes.
#[tokio::test]
async fn a_wait_returns_the_bytes_as_soon_as_they_arrive() {
    let root = scratch("wait-grow");
    let path = write(&root, "growing.log", b"first\n");

    let writer = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(120));
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("the fixture must stay appendable");
        file.write_all(b"second\n")
            .expect("the append must be written");
    });

    let started = std::time::Instant::now();
    let answer = read_waiting(&root, "growing.log", 6, 64, 5_000).await;
    let elapsed = started.elapsed();
    writer.join().expect("the writer thread must finish");

    match answer {
        Response::FileSlice {
            bytes,
            next,
            waited,
            ..
        } => {
            assert_eq!(bytes, b"second\n", "the appended bytes must be the answer");
            assert_eq!(next, 13);
            assert!(!waited, "bytes arrived, so this is not an empty wait");
        }
        other => panic!("expected a slice, got {other:?}"),
    }
    assert!(
        elapsed < std::time::Duration::from_millis(4_000),
        "it must return on the byte, not on the deadline: took {elapsed:?}"
    );

    std::fs::remove_dir_all(&root).expect("the fixture must be removable");
}

/// A wait is capped by the daemon, so no request can outlive it.
///
/// Checked on the arithmetic rather than by waiting: sleeping through the cap
/// would cost the suite thirty seconds to prove that sleeping works. The
/// property is that no value a client sends can produce a longer wait.
#[test]
fn a_wait_longer_than_the_cap_is_cut_to_the_cap() {
    let cap = std::time::Duration::from_millis(u64::from(crate::files::MAX_WAIT_MS));

    assert_eq!(crate::files::bounded_wait(u32::MAX), cap);
    assert_eq!(
        crate::files::bounded_wait(crate::files::MAX_WAIT_MS + 1),
        cap
    );
    assert_eq!(crate::files::bounded_wait(crate::files::MAX_WAIT_MS), cap);
    assert!(
        crate::files::bounded_wait(crate::files::MAX_WAIT_MS - 1) < cap,
        "a wait under the cap must be honoured as asked"
    );
    assert_eq!(crate::files::bounded_wait(0), std::time::Duration::ZERO);
}

/// A waiting read does not occupy the daemon: it is still answered.
///
/// This is the property that makes a bounded wait different from a hang, and it
/// needs a **single** worker to prove it: with two, a read that blocked the
/// worker would still be answered by the other one, and the test would pass
/// whatever the code did.
///
/// The clock starts before the reading task is spawned. Measuring after the
/// pause would hide the very delay being tested: a blocking read would have
/// finished during the pause, and the answer would look fast.
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn a_waiting_read_leaves_the_daemon_answering() {
    let root = scratch("wait-concurrent");
    write(&root, "quiet.log", b"x");

    let started = std::time::Instant::now();
    let waiting = {
        let root = root.clone();
        tokio::spawn(async move { read_waiting(&root, "quiet.log", 1, 8, 1_500).await })
    };

    // Give the read a chance to be inside its wait.
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    let answered = daemon(&root).handle(Request::HostInfo).await;
    let elapsed = started.elapsed();

    assert!(
        matches!(answered, Response::HostInfo(_)),
        "the daemon must answer while a read waits, got {answered:?}"
    );
    assert!(
        elapsed < std::time::Duration::from_millis(1_200),
        "the answer must not wait behind the read: took {elapsed:?}"
    );

    let waited = waiting.await.expect("the waiting task must finish");
    assert!(
        matches!(waited, Response::FileSlice { waited: true, .. }),
        "the waiting read must still report its wait, got {waited:?}"
    );

    std::fs::remove_dir_all(&root).expect("the fixture must be removable");
}

/// A rotated file is not followed into the file that replaced it.
///
/// The wait asks the *open handle* how large the file is, so after a rotation
/// the reader sees the old file stop growing. Asking the path instead would
/// report the new file's bytes and hand the reader a slice of a different file
/// at the same offset, which looks like data and is not.
#[tokio::test]
async fn a_rotated_file_is_not_followed_into_its_replacement() {
    let root = scratch("wait-rotate");
    write(&root, "app.log", b"old\n");

    let rotator = {
        let root = root.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(120));
            std::fs::rename(root.join("app.log"), root.join("app.log.1"))
                .expect("the rotation must rename the file");
            std::fs::write(root.join("app.log"), b"new and longer content\n")
                .expect("the replacement must be written");
        })
    };

    let answer = read_waiting(&root, "app.log", 4, 64, 600).await;
    rotator.join().expect("the rotator must finish");

    match answer {
        Response::FileSlice { waited, bytes, .. } => {
            assert!(
                waited,
                "the file it opened stopped growing, so it waited and found nothing"
            );
            assert!(
                bytes.is_empty(),
                "it must not return the replacement file's bytes, got {:?}",
                String::from_utf8_lossy(&bytes)
            );
        }
        other => panic!("expected a slice, got {other:?}"),
    }

    std::fs::remove_dir_all(&root).expect("the fixture must be removable");
}
