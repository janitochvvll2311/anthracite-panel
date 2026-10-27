//! Terminal contract tests against a real PTY.
//!
//! No mocks: `/bin/sh` runs for real, bytes cross a real descriptor, and the
//! registry is exercised with a clock the test moves by hand.

use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anth_panel::terminal::{OpenError, PtySession, Session, Sessions};
use anth_wire::TerminalSize;

fn size(cols: u16, rows: u16) -> TerminalSize {
    TerminalSize { cols, rows }
}

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

#[allow(dead_code)]
fn drain_until(sessions: &Sessions, id: &str, needle: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut seen = String::new();
    while Instant::now() < deadline {
        let chunk = futures_lite_block_on(sessions.read("test", id, 1_000)).expect("read");
        seen.push_str(&String::from_utf8_lossy(&chunk.bytes));
        if seen.contains(needle) {
            return seen;
        }
        if chunk.eof {
            break;
        }
    }
    seen
}

/// The test runtime is single-threaded per test; a tiny executor keeps the
/// registry's async read usable without pulling a second test harness in.
fn futures_lite_block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .expect("runtime")
        .block_on(future)
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
    session.write(b"printf '\\377\\376RAWMARK\\n'\n").unwrap();

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
    panic!("raw non-UTF-8 bytes never arrived");
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
    assert!(PtySession::spawn("/bin/sh", size(501, 24)).is_err());
    assert!(PtySession::spawn("/bin/sh", TerminalSize::MAX).is_ok());
}

#[test]
fn closing_a_session_kills_the_shell_process_group() {
    let sessions = Sessions::new(true, 4, Duration::ZERO);
    let id = sessions
        .open("test", "/bin/sh", size(80, 24))
        .expect("open");

    // The shell echoes the command; only the answer after `PID=` is the pid.
    sessions.write("test", &id, b"echo READY-PID=$$\n").unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let pid = loop {
        let chunk = futures_lite_block_on(sessions.read("test", &id, 1_000)).unwrap();
        let text = String::from_utf8_lossy(&chunk.bytes);
        let mut found = None;
        for rest in text.split("READY-PID=").skip(1) {
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if !digits.is_empty() {
                found = digits.parse::<u32>().ok();
                break;
            }
        }
        if let Some(pid) = found {
            break pid;
        }
        assert!(Instant::now() < deadline, "no pid in {text:?}");
    };
    assert!(std::path::Path::new(&format!("/proc/{pid}")).exists());

    sessions.close("test", &id).expect("close");
    assert!(sessions.is_empty());
    for _ in 0..100 {
        if !std::path::Path::new(&format!("/proc/{pid}")).exists() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("shell {pid} survived close: leaked process");
}

#[test]
fn a_quiet_read_does_not_advance_seq() {
    let sessions = Sessions::new(true, 2, Duration::ZERO);
    let id = sessions.open("test", "/bin/sh", size(80, 24)).unwrap();
    sessions.write("test", &id, b"echo FIRST\n").unwrap();
    let chunk = {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let chunk = futures_lite_block_on(sessions.read("test", &id, 1_000)).unwrap();
            if chunk.bytes.windows(5).any(|w| w == &b"FIRST"[..]) {
                break chunk;
            }
            assert!(Instant::now() < deadline, "no output");
        }
    };
    assert!(chunk.seq >= 1);
    let mut last_seq = chunk.seq;
    loop {
        let quiet = futures_lite_block_on(sessions.read("test", &id, 0)).unwrap();
        if quiet.bytes.is_empty() {
            assert_eq!(quiet.seq, last_seq, "an empty read must not advance seq");
            break;
        }
        assert!(quiet.seq > last_seq, "a non-empty read must advance seq");
        last_seq = quiet.seq;
    }
    sessions.close("test", &id).unwrap();
}

#[test]
fn the_registry_enforces_the_session_cap() {
    let sessions = Sessions::new(true, 2, Duration::ZERO);
    let first = sessions.open("test", "/bin/sh", size(80, 24)).unwrap();
    let _second = sessions.open("test", "/bin/sh", size(80, 24)).unwrap();
    match sessions.open("test", "/bin/sh", size(80, 24)) {
        Err(OpenError::AtCapacity) => {}
        other => panic!("expected AtCapacity, got {other:?}"),
    }
    sessions.close("test", &first).unwrap();
    assert!(sessions.open("test", "/bin/sh", size(80, 24)).is_ok());
}

#[test]
fn a_handle_is_invisible_to_another_owner() {
    let sessions = Sessions::new(true, 2, Duration::ZERO);
    let id = sessions.open("owner-a", "/bin/sh", size(80, 24)).unwrap();
    let error = sessions.write("owner-b", &id, b"id\n").unwrap_err();
    assert_eq!(error.kind, anth_wire::ErrorKind::NotFound);
    let error = futures_lite_block_on(sessions.read("owner-b", &id, 0)).unwrap_err();
    assert_eq!(error.kind, anth_wire::ErrorKind::NotFound);
    sessions.close("owner-a", &id).unwrap();
}

#[test]
fn an_idle_session_is_reaped_and_a_used_one_is_not() {
    let start = Instant::now();
    let clock = Arc::new(Mutex::new(start));
    let now = Arc::clone(&clock);
    let sessions = Sessions::with_clock(
        true,
        2,
        Duration::from_secs(10),
        Arc::new(move || *now.lock().unwrap()),
    );
    let idle = sessions.open("test", "/bin/sh", size(80, 24)).unwrap();
    let used = sessions.open("test", "/bin/sh", size(80, 24)).unwrap();

    // Use one session at +5s; both are now old at +15s, only the unused one
    // has passed its own idle window relative to its last use.
    *clock.lock().unwrap() = start + Duration::from_secs(5);
    sessions.write("test", &used, b"echo alive\n").unwrap();

    *clock.lock().unwrap() = start + Duration::from_secs(14);
    let removed = sessions.reap();
    assert_eq!(removed, 1);
    assert!(futures_lite_block_on(sessions.read("test", &idle, 0)).is_err());
    assert!(futures_lite_block_on(sessions.read("test", &used, 0)).is_ok());
    sessions.close("test", &used).unwrap();
}

#[test]
fn a_terminal_reports_the_exit_code() {
    let sessions = Sessions::new(true, 2, Duration::ZERO);
    let id = sessions.open("test", "/bin/sh", size(80, 24)).unwrap();
    sessions.write("test", &id, b"exit 7\n").unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let chunk = futures_lite_block_on(sessions.read("test", &id, 1_000)).unwrap();
        if chunk.eof {
            assert_eq!(chunk.exit_code, Some(7));
            return;
        }
        assert!(Instant::now() < deadline, "shell never exited");
    }
}

#[test]
fn a_missing_program_fails_to_spawn() {
    let sessions = Sessions::new(true, 2, Duration::ZERO);
    assert!(
        sessions
            .open("test", "/nonexistent/shell", size(80, 24))
            .is_err()
    );
}
