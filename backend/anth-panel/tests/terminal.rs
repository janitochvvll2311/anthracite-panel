//! Terminal tests against a real PTY and a real Unix socket.
//!
//! No mocks: `/bin/sh` runs for real and the frames cross an actual socket.

use std::io::Read;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anth_panel::config::{Config, TerminalConfig};
use anth_panel::terminal::{OpenError, Output, PtySession, Session, Sessions, Verdict, auth};
use anth_proto::{AnthClient, Request, Response, TerminalSize};
use tokio::net::UnixStream;

fn size(cols: u16, rows: u16) -> TerminalSize {
    TerminalSize { cols, rows }
}

fn unique_socket(name: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "anth-term-{name}-{}-{nanos}.sock",
        std::process::id()
    ))
}

/// Reads the PTY until `needle` appears or the deadline passes.
fn read_until(reader: &mut (dyn Read + Send), needle: &str, within: Duration) -> String {
    let deadline = Instant::now() + within;
    let mut seen = String::new();
    let mut buffer = [0u8; 4096];
    while Instant::now() < deadline {
        match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                seen.push_str(&String::from_utf8_lossy(&buffer[..n]));
                if seen.contains(needle) {
                    break;
                }
            }
        }
    }
    seen
}

/// Starts the daemon on a fresh socket and waits for it to bind.
async fn start_daemon(config: Config, path: &std::path::Path) {
    let _ = std::fs::remove_file(path);
    tokio::spawn(async move { anth_panel::daemon::serve(&config).await });
    for _ in 0..200 {
        if path.exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("daemon did not bind {}", path.display());
}

#[test]
fn a_real_shell_runs_and_reports_the_pty_size() {
    let (mut session, mut reader) = PtySession::spawn("/bin/sh", size(100, 30)).unwrap();
    assert_eq!(session.program(), "/bin/sh");

    session.write(b"stty size\n").unwrap();
    let out = read_until(reader.as_mut(), "30 100", Duration::from_secs(10));
    assert!(
        out.contains("30 100"),
        "expected the pty size, got: {out:?}"
    );
}

#[test]
fn the_shell_gets_a_utf8_locale() {
    let (mut session, mut reader) = PtySession::spawn("/bin/sh", size(80, 24)).unwrap();

    // `btop` and friends refuse to start without a UTF-8 locale. The echo of
    // this command contains the literal variables, so match on the value.
    session
        .write(b"printf '%s:%s\\n' \"$LANG\" \"$LC_ALL\"\n")
        .unwrap();
    let out = read_until(reader.as_mut(), "C.UTF-8:C.UTF-8", Duration::from_secs(10));
    assert!(
        out.contains("C.UTF-8:C.UTF-8"),
        "expected a UTF-8 locale, got: {out:?}"
    );
}

#[test]
fn terminal_output_carries_bytes_that_are_not_utf8() {
    let (mut session, mut reader) = PtySession::spawn("/bin/sh", size(80, 24)).unwrap();

    // 0xff and 0xfe are not valid UTF-8; the old line framing rejected them.
    session.write(b"printf '\\377\\376RAWMARK\\n'\n").unwrap();

    // Wait for the raw bytes themselves: the shell echoes the command text, so
    // matching on the marker alone could stop before the output arrives.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut seen = Vec::new();
    let mut buffer = [0u8; 4096];
    while Instant::now() < deadline {
        match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                seen.extend_from_slice(&buffer[..n]);
                if seen.windows(2).any(|w| w == [0xff, 0xfe]) {
                    return;
                }
            }
        }
    }
    panic!(
        "raw non-UTF-8 bytes never arrived, got: {:?}",
        String::from_utf8_lossy(&seen)
    );
}

#[test]
fn resizing_changes_the_size_the_shell_sees() {
    let (mut session, mut reader) = PtySession::spawn("/bin/sh", size(80, 24)).unwrap();
    session.resize(size(120, 40)).unwrap();

    session.write(b"stty size\n").unwrap();
    let out = read_until(reader.as_mut(), "40 120", Duration::from_secs(10));
    assert!(
        out.contains("40 120"),
        "expected the new size, got: {out:?}"
    );
}

#[test]
fn an_absurd_size_is_rejected_before_spawning() {
    assert!(PtySession::spawn("/bin/sh", size(0, 0)).is_err());
    assert!(PtySession::spawn("/bin/sh", size(5000, 24)).is_err());
    assert!(PtySession::spawn("/bin/sh", TerminalSize::MAX).is_ok());
}

#[tokio::test]
async fn closing_a_session_kills_the_shell_process() {
    let sessions = Sessions::new(4);
    let (id, _, _) = sessions.open("/bin/sh", size(80, 24)).await.unwrap();
    let mut receiver = sessions.take_output(id).await.unwrap();

    // The shell reports its own pid, so the assertion is about the real process
    // and not about bookkeeping inside the registry. Accumulate across chunks:
    // the echo and its digits can arrive in separate reads under load.
    let _ = sessions.write(id, b"echo PID=$$\n").await.unwrap();
    let mut output = String::new();
    let pid = 'read: loop {
        match tokio::time::timeout(Duration::from_secs(10), receiver.recv()).await {
            Ok(Some(Output::Data(bytes))) => {
                output.push_str(&String::from_utf8_lossy(&bytes));
                // The echoed command contains a literal `PID=$$`; only matches
                // followed by digits are the answer.
                for rest in output.split("PID=").skip(1) {
                    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                    if !digits.is_empty() {
                        break 'read digits;
                    }
                }
            }
            _ => panic!("shell never reported its pid"),
        }
    };

    let proc_path = format!("/proc/{pid}");
    assert!(
        std::path::Path::new(&proc_path).exists(),
        "shell {pid} should be running before close"
    );

    sessions.close(id).await;
    assert!(sessions.is_empty().await);

    for _ in 0..100 {
        if !std::path::Path::new(&proc_path).exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("shell {pid} survived close: leaked process");
}

#[tokio::test]
async fn the_registry_enforces_the_session_cap() {
    let sessions = Sessions::new(2);

    let (first, _, _) = sessions.open("/bin/sh", size(80, 24)).await.unwrap();
    let _second = sessions.open("/bin/sh", size(80, 24)).await.unwrap();

    match sessions.open("/bin/sh", size(80, 24)).await {
        Err(OpenError::AtCapacity) => {}
        other => panic!("expected AtCapacity, got {other:?}"),
    }

    sessions.close(first).await;
    assert!(
        sessions.open("/bin/sh", size(80, 24)).await.is_ok(),
        "a freed slot must be reusable"
    );
}

#[tokio::test]
async fn a_bad_size_is_refused_by_the_registry_too() {
    let sessions = Sessions::new(2);
    match sessions.open("/bin/sh", size(0, 0)).await {
        Err(OpenError::BadSize) => {}
        other => panic!("expected BadSize, got {other:?}"),
    }
}

#[test]
fn the_token_gate_separates_disabled_from_denied() {
    // Disabled must not look like "wrong token": a deployment that never
    // enabled the terminal should not reveal that it exists.
    assert_eq!(auth::check("", None), Verdict::Disabled);
    assert_eq!(auth::check("", Some("anything")), Verdict::Disabled);

    assert_eq!(auth::check("secret", None), Verdict::Denied);
    assert_eq!(auth::check("secret", Some("wrong")), Verdict::Denied);
    assert_eq!(auth::check("secret", Some("secret")), Verdict::Allowed);
}

#[test]
fn constant_time_comparison_matches_equality() {
    assert!(auth::constant_time_eq(b"abc", b"abc"));
    assert!(!auth::constant_time_eq(b"abc", b"abd"));
    assert!(!auth::constant_time_eq(b"abc", b"ab"));
    assert!(!auth::constant_time_eq(b"", b"a"));
    assert!(auth::constant_time_eq(b"", b""));
}

#[tokio::test]
async fn a_terminal_session_runs_over_the_socket() {
    let path = unique_socket("session");
    let config = Config {
        socket_path: path.to_string_lossy().into_owned(),
        terminal: TerminalConfig {
            shell: "/bin/sh".to_string(),
            token: "secret".to_string(),
            max_sessions: 2,
        },
    };
    start_daemon(config, &path).await;

    let stream = UnixStream::connect(&path).await.unwrap();
    let mut client = AnthClient::new(stream);

    client
        .send_request(&Request::TerminalOpen(anth_proto::TerminalOpenArgs {
            size: Some(size(90, 25)),
        }))
        .await
        .unwrap();

    let id = match client.receive_response().await.unwrap().unwrap() {
        Response::TerminalOpened(opened) => {
            assert_eq!(opened.size, size(90, 25));
            assert_eq!(opened.program, "/bin/sh");
            opened.id
        }
        other => panic!("expected TerminalOpened, got {other:?}"),
    };

    client
        .send_request(&Request::TerminalInput {
            id,
            bytes: b"echo MARKER-42\n".to_vec(),
        })
        .await
        .unwrap();

    // Output arrives unsolicited and interleaves with acknowledgements, so read
    // until the marker shows up in a TerminalOutput frame.
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut seen = Vec::new();
    while Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(5), client.receive_response()).await {
            Ok(Some(Ok(Response::TerminalOutput { id: out_id, bytes }))) => {
                assert_eq!(out_id, id);
                seen.extend_from_slice(&bytes);
                if String::from_utf8_lossy(&seen).contains("MARKER-42") {
                    break;
                }
            }
            Ok(Some(Ok(_))) => continue,
            Ok(Some(Err(e))) => panic!("malformed response: {e}"),
            Ok(None) => panic!("daemon closed the connection"),
            Err(_) => break,
        }
    }

    let text = String::from_utf8_lossy(&seen).into_owned();
    assert!(text.contains("MARKER-42"), "shell output missing: {text:?}");

    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn input_for_an_unknown_session_is_refused() {
    let path = unique_socket("unknown");
    let config = Config {
        socket_path: path.to_string_lossy().into_owned(),
        terminal: TerminalConfig::default(),
    };
    start_daemon(config, &path).await;

    let stream = UnixStream::connect(&path).await.unwrap();
    let mut client = AnthClient::new(stream);
    client
        .send_request(&Request::TerminalInput {
            id: 999,
            bytes: b"id\n".to_vec(),
        })
        .await
        .unwrap();

    match client.receive_response().await.unwrap().unwrap() {
        Response::Error { detail } => {
            assert!(detail.contains("not opened on this connection"), "{detail}");
        }
        other => panic!("expected Error, got {other:?}"),
    }

    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn a_client_that_disappears_does_not_leave_a_shell_running() {
    let path = unique_socket("abandon");
    let config = Config {
        socket_path: path.to_string_lossy().into_owned(),
        terminal: TerminalConfig {
            shell: "/bin/sh".to_string(),
            token: "secret".to_string(),
            max_sessions: 2,
        },
    };
    start_daemon(config, &path).await;

    let stream = UnixStream::connect(&path).await.unwrap();
    let mut client = AnthClient::new(stream);
    client
        .send_request(&Request::TerminalOpen(anth_proto::TerminalOpenArgs {
            size: Some(size(80, 24)),
        }))
        .await
        .unwrap();
    let id = match client.receive_response().await.unwrap().unwrap() {
        Response::TerminalOpened(opened) => opened.id,
        other => panic!("expected TerminalOpened, got {other:?}"),
    };

    client
        .send_request(&Request::TerminalInput {
            id,
            bytes: b"echo PID=$$\n".to_vec(),
        })
        .await
        .unwrap();

    // Learn the pid from the output, then vanish without sending TerminalClose.
    // Accumulate across frames: "PID=" and its digits can arrive in different
    // chunks, and matching per-chunk would miss the pid intermittently.
    let mut pid = String::new();
    let mut seen = String::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline && pid.is_empty() {
        match tokio::time::timeout(Duration::from_secs(5), client.receive_response()).await {
            Ok(Some(Ok(Response::TerminalOutput { bytes, .. }))) => {
                seen.push_str(&String::from_utf8_lossy(&bytes));
                // The echoed command contains a literal `PID=$$`, so only the
                // occurrences followed by digits are candidate answers.
                for rest in seen.split("PID=").skip(1) {
                    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                    if !digits.is_empty() {
                        pid = digits;
                    }
                }
            }
            // Input is acknowledged by silence; only output is expected.
            Ok(Some(Ok(Response::Ok { .. }))) => continue,
            Ok(Some(Ok(other))) => panic!("unexpected response while waiting for pid: {other:?}"),
            Ok(Some(Err(e))) => panic!("malformed response: {e}"),
            Ok(None) => panic!("daemon closed the connection"),
            Err(_) => break,
        }
    }
    assert!(
        !pid.is_empty(),
        "shell never reported its pid; frames seen: {seen:?}"
    );

    // Drop the socket mid-session: no close frame, no goodbye.
    drop(client);

    let proc_path = format!("/proc/{pid}");
    for _ in 0..150 {
        if !std::path::Path::new(&proc_path).exists() {
            let _ = std::fs::remove_file(&path);
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let _ = std::fs::remove_file(&path);
    panic!("shell {pid} outlived its client: the cleanup path was skipped");
}
