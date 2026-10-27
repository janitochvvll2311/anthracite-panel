//! Process table, signal guards and spawn guards against the live host.

use std::time::{Duration, Instant};

use anth_panel::process::ProcessService;
use anth_wire::{PageRequest, ProcessSignal};

fn service(control: bool, spawn: bool) -> ProcessService {
    ProcessService::new(control, spawn, vec!["/bin/sleep".to_string()])
}

fn page() -> PageRequest {
    PageRequest {
        limit: Some(1000),
        cursor: None,
    }
}

fn find(service: &ProcessService, pid: u32) -> anth_wire::ProcessInfo {
    let mut cursor = None;
    loop {
        let snapshot = service
            .snapshot(&PageRequest {
                limit: Some(1000),
                cursor,
            })
            .expect("snapshot");
        let next = snapshot.page.next.clone();
        if let Some(process) = snapshot
            .page
            .items
            .iter()
            .find(|process| process.pid == pid)
        {
            return process.clone();
        }
        match next {
            Some(value) => cursor = Some(value),
            None => panic!("the process is in the table"),
        }
    }
}

#[test]
fn a_snapshot_reports_the_live_table_and_its_policy() {
    let snapshot = service(false, false).snapshot(&page()).expect("snapshot");
    assert!(snapshot.clock_ticks > 0);
    assert!(snapshot.timestamp_ms > 0);
    assert!(!snapshot.control_enabled);
    assert!(!snapshot.spawn_enabled);
    assert_eq!(snapshot.spawn_allow, vec!["/bin/sleep".to_string()]);
    assert!(snapshot.page.total.unwrap_or(0) > 0);

    let own = find(&service(false, false), std::process::id());
    assert!(own.command.contains("process"));
    assert_eq!(own.uid, unsafe { libc::getuid() });
    assert!(!own.user.is_empty());
    assert!(own.start_ticks > 0);
}

#[test]
fn rows_are_sorted_by_cpu_and_paged_by_offset() {
    let service = service(false, false);
    let all = service.snapshot(&page()).expect("snapshot");
    let sorted = all
        .page
        .items
        .windows(2)
        .all(|pair| pair[0].cpu_ticks >= pair[1].cpu_ticks);
    assert!(sorted, "cpu_ticks must descend");

    let first = service
        .snapshot(&PageRequest {
            limit: Some(1),
            cursor: None,
        })
        .expect("first page");
    assert_eq!(first.page.items.len(), 1);
    let total = first.page.total.expect("total");
    if total > 1 {
        let cursor = first.page.next.clone().expect("next cursor");
        let second = service
            .snapshot(&PageRequest {
                limit: Some(1),
                cursor: Some(cursor),
            })
            .expect("second page");
        assert_eq!(second.page.items.len(), 1);
        assert_ne!(second.page.items[0].pid, first.page.items[0].pid);
    }
}

#[test]
fn a_bad_cursor_is_refused() {
    let service = service(false, false);
    let error = service
        .snapshot(&PageRequest {
            limit: Some(10),
            cursor: Some("not-a-number".to_string()),
        })
        .expect_err("bad cursor");
    assert_eq!(error.kind, anth_wire::ErrorKind::Invalid);
}

#[test]
fn signals_are_refused_without_control_or_for_protected_pids() {
    let own = std::process::id();
    let error = service(false, true)
        .signal(own, 0, ProcessSignal::Term, false)
        .expect_err("control off");
    assert_eq!(error.code, "process_control_disabled");

    for pid in [0, 1, own] {
        let error = service(true, false)
            .signal(pid, 1, ProcessSignal::Term, false)
            .expect_err("protected");
        assert!(
            ["protected_process", "no process"].contains(&error.code.as_str()),
            "unexpected code {} for {pid}",
            error.code
        );
    }
}

#[test]
fn a_recycled_pid_is_refused_by_start_ticks() {
    let service = service(true, true);
    let child = std::process::Command::new("/bin/sleep")
        .arg("30")
        .spawn()
        .expect("spawn");
    let pid = child.id();
    let process = find(&service, pid);

    let error = service
        .signal(pid, process.start_ticks + 1, ProcessSignal::Term, false)
        .expect_err("stale start_ticks");
    assert_eq!(error.code, "process_changed");

    service
        .signal(pid, process.start_ticks, ProcessSignal::Term, false)
        .expect("signal");
    // The test process owns the child, so wait for it to avoid a zombie.
    let status = child.wait_with_output().expect("wait");
    assert!(!status.status.success());
}

#[test]
fn a_group_signal_requires_the_process_to_lead_its_group() {
    let service = service(true, true);
    let mut child = std::process::Command::new("/bin/sleep")
        .arg("30")
        .spawn()
        .expect("spawn");
    let pid = child.id();
    let process = find(&service, pid);

    let error = service
        .signal(pid, process.start_ticks, ProcessSignal::Stop, true)
        .expect_err("not a group leader");
    assert_eq!(error.code, "not_group_leader");

    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn spawn_refuses_what_the_policy_does_not_allow() {
    let cases = [
        (
            ProcessService::new(false, false, vec!["/bin/sleep".to_string()]),
            "/bin/sleep",
            "process_spawn_disabled",
        ),
        (
            ProcessService::new(false, true, vec!["/bin/true".to_string()]),
            "/bin/sleep",
            "program_not_allowed",
        ),
        (
            ProcessService::new(false, true, vec![]),
            "/bin/sleep",
            "program_not_allowed",
        ),
        (
            ProcessService::new(false, true, vec!["/bin/sleep".to_string()]),
            "sleep",
            "program_not_absolute",
        ),
        (
            ProcessService::new(false, true, vec!["/bin/sleep".to_string()]),
            "/definitely/not/here",
            "not_found",
        ),
    ];
    for (service, program, code) in cases {
        let error = service
            .spawn(program, &[], None)
            .expect_err("refused spawn");
        assert_eq!(error.code, code, "for {program}");
    }
}

#[test]
fn spawn_refuses_a_relative_or_missing_working_directory() {
    let service = ProcessService::new(false, true, vec!["/bin/sleep".to_string()]);
    let relative = service
        .spawn("/bin/sleep", &["1".to_string()], Some("tmp"))
        .expect_err("relative cwd");
    assert_eq!(relative.code, "cwd_not_absolute");

    let missing = service
        .spawn(
            "/bin/sleep",
            &["1".to_string()],
            Some("/definitely/not/a/directory"),
        )
        .expect_err("missing cwd");
    assert_eq!(missing.code, "cwd_not_found");
}

#[test]
fn spawn_starts_an_allowlisted_program_and_reaps_it() {
    let service = ProcessService::new(false, true, vec!["/bin/sleep".to_string()]);
    let started = service
        .spawn("/bin/sleep", &["1".to_string()], None)
        .expect("spawn");
    assert!(started.pid > 0);
    assert_eq!(started.program, "/bin/sleep");

    // The spawn thread reaps the child; once it exits, `/proc` drops the entry.
    let deadline = Instant::now() + Duration::from_secs(5);
    while std::path::Path::new(&format!("/proc/{}", started.pid)).exists() {
        assert!(
            Instant::now() < deadline,
            "the spawned process was not reaped"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}
