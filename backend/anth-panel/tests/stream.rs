use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anth_panel::stream::{Frames, MIN_INTERVAL, follow, parse_interval};
use anth_proto::{AnthServer, FileError, FileErrorKind, Request, Response};
use std::process::ExitCode;

#[test]
fn parse_interval_accepts_every_unit() {
    assert_eq!(parse_interval("500ms").unwrap(), Duration::from_millis(500));
    assert_eq!(parse_interval("2s").unwrap(), Duration::from_secs(2));
    assert_eq!(parse_interval("1m").unwrap(), Duration::from_secs(60));
    assert_eq!(parse_interval("2h").unwrap(), Duration::from_secs(7200));
}

#[test]
fn parse_interval_accepts_fractional_values_and_surrounding_space() {
    assert_eq!(parse_interval("1.5s").unwrap(), Duration::from_millis(1500));
    assert_eq!(
        parse_interval(" 250ms ").unwrap(),
        Duration::from_millis(250)
    );
    assert_eq!(parse_interval("0.5m").unwrap(), Duration::from_secs(30));
}

#[test]
fn parse_interval_rejects_a_missing_unit() {
    let err = parse_interval("2").unwrap_err();
    assert!(err.contains("add a unit suffix"), "{err}");
}

#[test]
fn parse_interval_rejects_non_numeric_and_negative_values() {
    assert!(parse_interval("abc s").is_err());
    assert!(parse_interval("-1s").is_err());
    assert!(parse_interval("0s").is_err());
    assert!(parse_interval("NaNs").is_err());
    assert!(parse_interval("infs").is_err());
}

#[test]
fn parse_interval_rejects_a_busy_loop() {
    let err = parse_interval("1ms").unwrap_err();
    assert!(err.contains("below the 10ms minimum"), "{err}");

    // The floor is inclusive.
    assert_eq!(parse_interval("10ms").unwrap(), MIN_INTERVAL);
}

#[test]
fn parse_interval_rejects_absurdly_long_periods() {
    assert!(parse_interval("100h").is_err());
    assert!(parse_interval("24h").is_ok());
}

/// Records what a command would print, so frame layout is testable without a TTY.
#[derive(Clone, Default)]
struct SharedBuffer(Arc<Mutex<Vec<u8>>>);

impl SharedBuffer {
    fn contents(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}

impl std::io::Write for SharedBuffer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn frames_are_separated_by_a_blank_line_when_redirected() {
    let sink = SharedBuffer::default();
    let mut frames = Frames::new(sink.clone(), false);

    frames.write_frame("first").unwrap();
    frames.write_frame("second").unwrap();

    assert_eq!(frames_count(&sink.contents()), 2);
    assert_eq!(sink.contents(), "first\n\nsecond\n");
}

#[test]
fn frames_overwrite_the_previous_one_on_a_terminal() {
    let sink = SharedBuffer::default();
    let mut frames = Frames::new(sink.clone(), true);

    frames.write_frame("one\ntwo\nthree").unwrap();
    frames.write_frame("next").unwrap();

    let written = sink.contents();
    // Three lines were written, so the cursor moves up three and clears below.
    assert!(written.contains("\x1b[3A\x1b[J"), "{written:?}");
    assert!(
        !written.contains("\n\n"),
        "no blank separator on a TTY: {written:?}"
    );
    assert!(written.ends_with("next\n"), "{written:?}");
}

#[test]
fn frames_count_trailing_line_breaks_in_the_height() {
    let sink = SharedBuffer::default();
    let mut frames = Frames::new(sink.clone(), true);

    frames.write_frame("a\nb").unwrap();
    frames.write_frame("c").unwrap();

    // "a\nb" is two lines, not three.
    assert!(
        sink.contents().contains("\x1b[2A\x1b[J"),
        "{:?}",
        sink.contents()
    );
}

fn frames_count(output: &str) -> usize {
    output.lines().filter(|line| !line.is_empty()).count()
}

#[tokio::test]
async fn follow_returns_failure_for_a_file_error_response() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "anth-stream-file-error-{}-{nanos}.sock",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let listener = tokio::net::UnixListener::bind(&path).unwrap();

    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let mut server = AnthServer::new(socket);
        assert!(matches!(
            server.receive_request().await,
            Some(Ok(Request::FileList { .. }))
        ));
        server
            .send_response(&Response::FileError(FileError::new(
                FileErrorKind::NotFound,
                "missing",
            )))
            .await
            .unwrap();
    });

    let code = follow(
        &path,
        &Request::FileList {
            path: "/".to_string(),
        },
        None,
        |_| String::new(),
    )
    .await;

    assert_eq!(code, ExitCode::FAILURE);
    server.await.unwrap();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn follow_returns_failure_for_a_volume_error_response() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "anth-stream-volume-error-{}-{nanos}.sock",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let listener = tokio::net::UnixListener::bind(&path).unwrap();

    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let mut server = AnthServer::new(socket);
        assert!(matches!(
            server.receive_request().await,
            Some(Ok(Request::VolumeUnmount { .. }))
        ));
        server
            .send_response(&Response::VolumeError(anth_proto::VolumeError::new(
                anth_proto::VolumeErrorKind::Busy,
                "target is busy",
            )))
            .await
            .unwrap();
    });

    let code = follow(
        &path,
        &Request::VolumeUnmount {
            target: "/mnt/data".to_string(),
        },
        None,
        |_| String::new(),
    )
    .await;

    assert_eq!(code, ExitCode::FAILURE);
    server.await.unwrap();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn follow_returns_failure_for_a_spawn_error_response() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "anth-stream-spawn-error-{}-{nanos}.sock",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let listener = tokio::net::UnixListener::bind(&path).unwrap();

    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let mut server = AnthServer::new(socket);
        assert!(matches!(
            server.receive_request().await,
            Some(Ok(Request::ProcessSpawn { .. }))
        ));
        server
            .send_response(&Response::ProcessSpawnError(
                anth_proto::ProcessSpawnError {
                    kind: anth_proto::ProcessSpawnErrorKind::NotAllowed,
                    detail: "`/bin/sh` is not in the spawn allowlist".to_string(),
                },
            ))
            .await
            .unwrap();
    });

    let code = follow(
        &path,
        &Request::ProcessSpawn {
            program: "/bin/sh".to_string(),
            args: Vec::new(),
            cwd: None,
        },
        None,
        |_| String::new(),
    )
    .await;

    assert_eq!(code, ExitCode::FAILURE);
    server.await.unwrap();
    let _ = std::fs::remove_file(&path);
}
