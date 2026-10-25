//! The session registry, over real processes.
//!
//! The rules about ownership, caps and expiry need no PTY, but the ones about
//! bytes do: a fake session would prove that the fake works. Each test opens a
//! real `sh`, drives it, and closes it, so a leak shows up as a process still
//! running rather than as a missing assertion.

use std::time::{Duration, Instant};

use anth_wire::ErrorKind;

use super::*;

/// A session registry whose clock the test moves by hand.
fn clocked() -> (Arc<Sessions>, Arc<Mutex<Instant>>) {
    let now = Arc::new(Mutex::new(Instant::now()));
    let read = Arc::clone(&now);
    let sessions = Sessions::with_clock(
        Box::new(move || *read.lock().unwrap_or_else(|e| e.into_inner())),
        Duration::from_secs(60),
        2,
    );
    (Arc::new(sessions), now)
}

fn advance(clock: &Arc<Mutex<Instant>>, by: Duration) {
    let mut now = clock.lock().unwrap_or_else(|e| e.into_inner());
    *now += by;
}

/// Reads until `needle` appears or the budget runs out.
///
/// A terminal is asynchronous, so a test that read once would be racing the
/// shell. Polling with a deadline is the honest way to wait for output.
fn read_until(sessions: &Sessions, id: &str, needle: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut seen = String::new();
    while Instant::now() < deadline {
        let chunk = sessions
            .read("test", id, 200)
            .expect("a live session must be readable");
        seen.push_str(&String::from_utf8_lossy(&chunk.bytes));
        if seen.contains(needle) {
            return seen;
        }
    }
    panic!("`{needle}` never appeared; saw {seen:?}");
}

#[test]
fn a_session_echoes_what_is_typed_into_it() {
    let sessions = Sessions::new();
    let id = sessions
        .open("test", "sh", 80, 24)
        .expect("a shell must be startable");

    sessions
        .write("test", &id, b"echo anth-marker\n")
        .expect("a write must reach the shell");

    let seen = read_until(&sessions, &id, "anth-marker");
    assert!(
        seen.contains("anth-marker"),
        "the shell's answer must come back, saw {seen:?}"
    );

    sessions.close("test", &id).expect("closing must succeed");
    assert!(
        sessions.len() == 0,
        "the registry must be empty after close"
    );
}

#[test]
fn seq_increases_only_when_bytes_arrive() {
    let sessions = Sessions::new();
    let id = sessions.open("test", "sh", 80, 24).expect("a shell");
    std::thread::sleep(Duration::from_millis(200));

    // Drain whatever the shell wrote on startup.
    let mut last = 0;
    for _ in 0..20 {
        let chunk = sessions.read("test", &id, 50).expect("a read");
        if chunk.bytes.is_empty() {
            break;
        }
        assert!(chunk.seq > last, "a non-empty read must advance seq");
        last = chunk.seq;
    }

    let quiet = sessions.read("test", &id, 100).expect("a read");
    assert!(quiet.bytes.is_empty(), "a quiet shell returns nothing");
    assert_eq!(
        quiet.seq, last,
        "an empty read must not advance seq, or a gap would be invisible"
    );

    sessions.close("test", &id).expect("close");
}

#[test]
fn a_handle_from_another_owner_is_answered_as_missing() {
    let sessions = Sessions::new();
    let id = sessions.open("owner-a", "sh", 80, 24).expect("a shell");

    let refused = sessions.read("owner-b", &id, 10).expect_err("must refuse");
    assert_eq!(refused.kind, ErrorKind::NotFound);
    // Both are the *same* error shape, naming the handle the caller asked
    // about. A different message for "someone else's" would let a caller probe
    // for handles that exist.
    let unknown = sessions
        .read("owner-b", "t999", 10)
        .expect_err("must refuse");
    assert_eq!(unknown.kind, ErrorKind::NotFound);
    assert!(
        refused.message.starts_with("no terminal") && unknown.message.starts_with("no terminal"),
        "both must read as missing, got {:?} and {:?}",
        refused.message,
        unknown.message
    );

    sessions.close("owner-a", &id).expect("close");
}

#[test]
fn the_cap_refuses_the_next_session_and_closing_one_makes_room() {
    // The cap under test is the registry's own, not `MAX_SESSIONS`: a test that
    // opened eight shells to reach the limit would be slow and would say
    // nothing about the rule.
    let (sessions, _clock) = clocked();
    let sessions = sessions.as_ref();
    let first = sessions.open("test", "sh", 80, 24).expect("first");
    let second = sessions.open("test", "sh", 80, 24).expect("second");

    let refused = sessions
        .open("test", "sh", 80, 24)
        .expect_err("the cap must bite");
    assert_eq!(refused.kind, ErrorKind::Conflict);
    assert!(
        refused.message.contains("close one first"),
        "the refusal must say what to do: {}",
        refused.message
    );

    sessions.close("test", &first).expect("close");
    let third = sessions
        .open("test", "sh", 80, 24)
        .expect("a closed session must make room");
    sessions.close("test", &second).expect("close");
    sessions.close("test", &third).expect("close");
}

#[test]
fn an_idle_session_is_reaped_and_a_used_one_is_not() {
    let (sessions, clock) = clocked();
    let used = sessions.open("test", "sh", 80, 24).expect("a shell");
    let idle = sessions.open("test", "sh", 80, 24).expect("a shell");

    // Keep one of them in use while time passes for both.
    advance(&clock, Duration::from_secs(40));
    sessions
        .read("test", &used, 50)
        .expect("a read keeps it alive");

    advance(&clock, Duration::from_secs(30));
    let reaped = sessions.reap();

    assert_eq!(reaped, 1, "only the untouched session must expire");
    sessions
        .read("test", &idle, 10)
        .expect_err("the idle one is gone");
    sessions
        .read("test", &used, 50)
        .expect("the used one must survive");

    sessions.close("test", &used).expect("close");
}

#[test]
fn a_program_outside_the_allowlist_is_refused_by_name() {
    let sessions = Sessions::new();
    let refused = sessions
        .open("test", "/bin/sh", 80, 24)
        .expect_err("a path must be refused");

    assert_eq!(refused.kind, ErrorKind::Forbidden);
    assert!(
        refused.message.contains("/bin/sh") && refused.message.contains("sh"),
        "the message must name the refusal and the alternatives: {}",
        refused.message
    );
    assert!(sessions.len() == 0, "a refusal must not leave a session");
}

#[test]
fn a_size_outside_the_bounds_is_refused_before_anything_starts() {
    let sessions = Sessions::new();
    for (cols, rows) in [
        (0u16, 24u16),
        (80, 0),
        (MAX_DIMENSION + 1, 24),
        (80, MAX_DIMENSION + 1),
    ] {
        let refused = sessions
            .open("test", "sh", cols, rows)
            .expect_err("a bad size must be refused");
        assert_eq!(refused.kind, ErrorKind::Invalid, "{cols}x{rows}");
    }
    assert_eq!(
        sessions.len(),
        0,
        "a refused size must not have started a process"
    );
}

#[test]
fn closing_a_session_drops_the_registry_entry() {
    let sessions = Sessions::new();
    let id = sessions.open("test", "sh", 80, 24).expect("a shell");

    sessions.close("test", &id).expect("close");
    assert_eq!(sessions.len(), 0);
    sessions
        .read("test", &id, 10)
        .expect_err("a closed handle must be unknown");
}

/// A child must not inherit the master half.
///
/// Without close-on-exec the shell keeps a copy of `/dev/pts/ptmx`, so the
/// terminal is never fully closed: the daemon can be killed and the shell stays
/// alive on a pts nobody reads, reparented to init. That is a leak nobody sees
/// until the machine accumulates shells, so it is pinned here.
#[test]
fn a_child_does_not_inherit_the_master_half() {
    let sessions = Sessions::new();
    let id = sessions
        .open("test", "sh", 80, 24)
        .expect("a shell must start");

    // The count goes to a file rather than back through the terminal: the shell
    // echoes the command it runs, so any marker read from the terminal appears
    // in the echo before the command has finished, and a test that waited for
    // it would assert against its own input.
    let report = std::env::temp_dir().join(format!(
        "anth-fd-count-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let report = report.to_string_lossy().replace('"', "");
    sessions
        .write(
            "test",
            &id,
            format!("ls -l /proc/self/fd | grep -c ptmx > '{report}'\n").as_bytes(),
        )
        .expect("a write");

    let deadline = Instant::now() + Duration::from_secs(5);
    let count = loop {
        if let Ok(text) = std::fs::read_to_string(&report)
            && let Ok(count) = text.trim().parse::<u32>()
        {
            break count;
        }
        assert!(
            Instant::now() < deadline,
            "the child never reported a count"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    let _ = std::fs::remove_file(&report);

    assert_eq!(
        count, 0,
        "the child must hold no descriptor on /dev/pts/ptmx"
    );

    sessions.close("test", &id).expect("close");
}
