//! Local WebSocket contract for terminal sessions.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anth_client::Client;
use anth_panel::config::{Config, TerminalConfig};
use anth_panel::dispatch::Daemon;
use anth_panel::log::{Logger, Settings};
use anth_panel::socket;
use anth_wire::{MAX_TERMINAL_INPUT, TerminalClientMessage, TerminalServerMessage, TerminalSize};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

mod common;

fn scratch(name: &str) -> (PathBuf, PathBuf) {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = PathBuf::from("/tmp/anthracite-panel/tests").join(format!(
        "terminal-socket-{name}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    (dir.join("anth.sock"), dir.join("config.toml"))
}

async fn start(enabled: bool) -> (PathBuf, Arc<Daemon>, tokio::task::JoinHandle<()>) {
    let (socket_path, config_path) = scratch(if enabled { "on" } else { "off" });
    let config = Config {
        socket_path: socket_path.to_string_lossy().into_owned(),
        terminal: TerminalConfig {
            enabled,
            shell: "/bin/sh".to_string(),
            max_sessions: 2,
            idle_seconds: 900,
        },
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
    // The listener is bound before `serve` starts, so the path exists already.
    (socket_path, daemon, handle)
}

async fn connect(path: &std::path::Path) -> Client {
    let client = Client::unix(path);
    client.hello().await.expect("hello");
    client
}

fn size() -> TerminalSize {
    TerminalSize { cols: 80, rows: 24 }
}

#[tokio::test]
async fn a_local_terminal_echoes_and_reports_the_exit_code() {
    let (path, daemon, server) = start(true).await;
    let client = connect(&path).await;
    let mut session = client.terminal(size()).await.expect("open");

    match session.next().await.expect("ready") {
        Some(TerminalServerMessage::Ready { shell, .. }) => assert_eq!(shell, "/bin/sh"),
        other => panic!("expected ready, got {other:?}"),
    }

    session.input(b"echo LOCAL-MARKER\n").await.expect("write");

    let mut seen = String::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        match session.next().await.expect("read") {
            Some(TerminalServerMessage::Output { data }) => {
                seen.push_str(&String::from_utf8_lossy(&STANDARD.decode(data).unwrap()));
                if seen.contains("LOCAL-MARKER") {
                    break;
                }
            }
            Some(TerminalServerMessage::Exit { code }) => {
                panic!("the shell exited early with {code}: {seen:?}")
            }
            Some(_) => {}
            None => panic!("the daemon closed the terminal: {seen:?}"),
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "no marker: {seen:?}"
        );
    }

    session.input(b"exit 3\n").await.expect("write exit");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        match session.next().await.expect("read") {
            Some(TerminalServerMessage::Exit { code }) => {
                assert_eq!(code, 3);
                break;
            }
            Some(_) => {}
            None => panic!("the daemon closed before the exit code"),
        }
        assert!(tokio::time::Instant::now() < deadline, "no exit");
    }

    session.close().await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !daemon.terminals().expect("real backends").is_empty() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the session was not released"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    server.abort();
}

#[tokio::test]
async fn a_disabled_terminal_refuses_the_upgrade() {
    let (path, daemon, server) = start(false).await;
    let client = connect(&path).await;
    let error = client.terminal(size()).await.expect_err("disabled");
    assert_eq!(error.code, "terminal_disabled");
    assert!(daemon.terminals().expect("real backends").is_empty());
    server.abort();
}

#[tokio::test]
async fn a_bad_size_is_refused_before_the_upgrade() {
    let (path, _daemon, server) = start(true).await;
    let client = connect(&path).await;
    let error = client
        .terminal(TerminalSize { cols: 0, rows: 24 })
        .await
        .expect_err("bad size");
    assert_eq!(error.code, "invalid");
    server.abort();
}

#[tokio::test]
async fn a_large_write_is_measured_before_it_reaches_the_pty() {
    let (path, _daemon, server) = start(true).await;
    let client = connect(&path).await;
    let mut session = client.terminal(size()).await.expect("open");
    match session.next().await.expect("ready") {
        Some(TerminalServerMessage::Ready { .. }) => {}
        other => panic!("expected ready, got {other:?}"),
    }

    session
        .send(&TerminalClientMessage::Input {
            data: STANDARD.encode(vec![b'x'; MAX_TERMINAL_INPUT + 1]),
        })
        .await
        .expect("send");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        match session.next().await.expect("read") {
            Some(TerminalServerMessage::Error { detail }) => {
                assert!(detail.contains("limit"), "{detail}");
                break;
            }
            Some(_) => {}
            None => panic!("the daemon closed before the error"),
        }
        assert!(tokio::time::Instant::now() < deadline, "no error");
    }
    server.abort();
}
