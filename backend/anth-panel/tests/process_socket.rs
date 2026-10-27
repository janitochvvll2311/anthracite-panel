//! Socket contract for the process table, signals and spawn.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anth_panel::client::Client;
use anth_panel::config::Config;
use anth_panel::dispatch::Daemon;
use anth_panel::log::{Logger, Settings};
use anth_panel::socket;
use anth_wire::{ProcessSignal, Request, Response};

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

async fn start(name: &str) -> (PathBuf, Arc<Daemon>, tokio::task::JoinHandle<()>) {
    let (socket_path, config_path) = scratch(name);
    let config = Config {
        socket_path: socket_path.to_string_lossy().into_owned(),
        ..Config::dev_default()
    };
    let paths = config.paths(&config_path).unwrap();
    let logger = Arc::new(Logger::buffer(Settings::default()).0);
    let daemon = Arc::new(Daemon::new(config, paths, logger));
    let listener = socket::bind(&socket_path).unwrap();
    let serve_daemon = Arc::clone(&daemon);
    let handle = tokio::spawn(async move {
        let _ = socket::serve(listener, serve_daemon).await;
    });
    (socket_path, daemon, handle)
}

async fn connect(path: &std::path::Path) -> Client {
    let mut client = Client::connect(path).await.expect("connect");
    client.hello().await.expect("hello");
    client
}

#[tokio::test]
async fn a_socket_snapshot_pages_signals_and_spawns() {
    let (path, _daemon, server) = start("all").await;
    let mut client = connect(&path).await;

    let page = match client
        .call(&Request::Processes {
            limit: Some(3),
            cursor: None,
        })
        .await
        .expect("processes")
    {
        Response::Processes(snapshot) => snapshot,
        other => panic!("expected a snapshot, got {other:?}"),
    };
    assert!(page.page.items.len() <= 3);
    assert!(page.page.total.unwrap_or(0) > 0);
    assert!(page.control_enabled);

    let started = match client
        .call(&Request::ProcessSpawn {
            program: "/bin/sleep".to_string(),
            args: vec!["30".to_string()],
            cwd: None,
        })
        .await
        .expect("spawn")
    {
        Response::ProcessStarted(started) => started,
        other => panic!("expected a started process, got {other:?}"),
    };

    let process = find_process(&mut client, started.pid).await;

    let stale = client
        .call(&Request::ProcessSignal {
            pid: started.pid,
            start_ticks: process.start_ticks + 1,
            signal: ProcessSignal::Term,
            group: false,
        })
        .await
        .expect_err("stale signal");
    assert_eq!(stale.code, "process_changed");

    let signalled = client
        .call(&Request::ProcessSignal {
            pid: started.pid,
            start_ticks: process.start_ticks,
            signal: ProcessSignal::Kill,
            group: false,
        })
        .await
        .expect("signal");
    assert!(matches!(signalled, Response::ProcessSignalled { .. }));

    let deadline = Instant::now() + Duration::from_secs(5);
    while std::path::Path::new(&format!("/proc/{}", started.pid)).exists() {
        assert!(Instant::now() < deadline, "the process did not exit");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let denied = client
        .call(&Request::ProcessSpawn {
            program: "/bin/sh".to_string(),
            args: Vec::new(),
            cwd: None,
        })
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
    let daemon = Arc::new(Daemon::new(config, paths, logger));
    let listener = socket::bind(&socket_path).unwrap();
    let server = tokio::spawn(async move {
        let _ = socket::serve(listener, daemon).await;
    });
    let mut client = connect(&socket_path).await;

    let listing = client
        .call(&Request::Processes {
            limit: None,
            cursor: None,
        })
        .await
        .expect("processes");
    match listing {
        Response::Processes(snapshot) => {
            assert!(!snapshot.control_enabled);
            assert!(!snapshot.spawn_enabled);
            assert!(snapshot.spawn_allow.is_empty());
        }
        other => panic!("expected a snapshot, got {other:?}"),
    }

    let signal = client
        .call(&Request::ProcessSignal {
            pid: 2,
            start_ticks: 1,
            signal: ProcessSignal::Term,
            group: false,
        })
        .await
        .expect_err("signal");
    assert_eq!(signal.code, "process_control_disabled");

    let spawn = client
        .call(&Request::ProcessSpawn {
            program: "/bin/sleep".to_string(),
            args: vec!["1".to_string()],
            cwd: None,
        })
        .await
        .expect_err("spawn");
    assert_eq!(spawn.code, "process_spawn_disabled");

    server.abort();
}
async fn find_process(client: &mut Client, pid: u32) -> anth_wire::ProcessInfo {
    let mut cursor = None;
    loop {
        let snapshot = match client
            .call(&Request::Processes {
                limit: Some(1000),
                cursor,
            })
            .await
            .expect("processes")
        {
            Response::Processes(snapshot) => snapshot,
            other => panic!("expected a snapshot, got {other:?}"),
        };
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
