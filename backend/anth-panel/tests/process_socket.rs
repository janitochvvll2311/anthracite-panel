//! Local HTTP contract for the process table, signals and spawn.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anth_client::Client;
use anth_panel::config::Config;
use anth_panel::dispatch::Daemon;
use anth_panel::log::{Logger, Settings};
use anth_panel::socket;
use anth_wire::ProcessSignal;

mod common;

fn scratch(name: &str) -> (PathBuf, PathBuf) {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = PathBuf::from("/tmp/anthracite-panel/tests").join(format!(
        "process-socket-{name}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    (dir.join("anth.sock"), dir.join("config.toml"))
}

async fn start(name: &str) -> (PathBuf, tokio::task::JoinHandle<()>) {
    let (socket_path, config_path) = scratch(name);
    let config = Config {
        socket_path: socket_path.to_string_lossy().into_owned(),
        ..Config::dev_default()
    };
    let paths = config.paths(&config_path).unwrap();
    let logger = Arc::new(Logger::buffer(Settings::default()).0);
    let daemon = Arc::new(Daemon::new(config, paths.clone(), logger));
    let listener = socket::bind(&socket_path).unwrap();
    let router = common::local_router(Arc::clone(&daemon), &paths);
    let handle = tokio::spawn(async move {
        let _ = socket::serve(listener, router).await;
    });
    (socket_path, handle)
}

async fn connect(path: &std::path::Path) -> Client {
    let client = Client::unix(path);
    client.hello().await.expect("hello");
    client
}

#[tokio::test]
async fn a_local_snapshot_pages_signals_and_spawns() {
    let (path, server) = start("all").await;
    let client = connect(&path).await;

    let snapshot = client.processes(Some(3), None).await.expect("processes");
    assert!(snapshot.page.items.len() <= 3);
    assert!(snapshot.page.total.unwrap_or(0) > 0);
    assert!(snapshot.control_enabled);

    let started = client
        .process_spawn("/bin/sleep", &["30".to_string()], None)
        .await
        .expect("spawn");

    let process = find_process(&client, started.pid).await;

    let stale = client
        .process_signal(
            started.pid,
            process.start_ticks + 1,
            ProcessSignal::Term,
            false,
        )
        .await
        .expect_err("stale signal");
    assert_eq!(stale.code, "process_changed");

    client
        .process_signal(started.pid, process.start_ticks, ProcessSignal::Kill, false)
        .await
        .expect("signal");

    let deadline = Instant::now() + Duration::from_secs(5);
    while std::path::Path::new(&format!("/proc/{}", started.pid)).exists() {
        assert!(Instant::now() < deadline, "the process did not exit");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let denied = client
        .process_spawn("/bin/sh", &[], None)
        .await
        .expect_err("denied spawn");
    assert_eq!(denied.code, "program_not_allowed");

    server.abort();
}

#[tokio::test]
async fn process_requests_are_rejected_when_the_switches_are_off() {
    let (socket_path, config_path) = scratch("off");
    let config = Config {
        socket_path: socket_path.to_string_lossy().into_owned(),
        process: anth_panel::config::ProcessConfig {
            control: false,
            spawn: false,
            spawn_allow: Vec::new(),
        },
        ..Config::default()
    };
    let paths = config.paths(&config_path).unwrap();
    let logger = Arc::new(Logger::buffer(Settings::default()).0);
    let daemon = Arc::new(Daemon::new(config, paths.clone(), logger));
    let listener = socket::bind(&socket_path).unwrap();
    let router = common::local_router(Arc::clone(&daemon), &paths);
    let server = tokio::spawn(async move {
        let _ = socket::serve(listener, router).await;
    });
    let client = connect(&socket_path).await;

    let snapshot = client.processes(None, None).await.expect("processes");
    assert!(!snapshot.control_enabled);
    assert!(!snapshot.spawn_enabled);
    assert!(snapshot.spawn_allow.is_empty());

    let signal = client
        .process_signal(2, 1, ProcessSignal::Term, false)
        .await
        .expect_err("signal");
    assert_eq!(signal.code, "process_control_disabled");

    let spawn = client
        .process_spawn("/bin/sleep", &["1".to_string()], None)
        .await
        .expect_err("spawn");
    assert_eq!(spawn.code, "process_spawn_disabled");

    server.abort();
}

async fn find_process(client: &Client, pid: u32) -> anth_wire::ProcessInfo {
    let mut cursor = None;
    loop {
        let snapshot = client
            .processes(Some(1000), cursor.as_deref())
            .await
            .expect("processes");
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
            None => panic!("the spawned process is not in the table"),
        }
    }
}
