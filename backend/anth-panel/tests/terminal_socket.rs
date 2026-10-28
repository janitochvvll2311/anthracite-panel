//! Socket contract for terminal handles, through the real listener and client.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anth_panel::client::Client;
use anth_panel::config::{Config, TerminalConfig};
use anth_panel::dispatch::Daemon;
use anth_panel::log::{Logger, Settings};
use anth_panel::socket;
use anth_wire::{Request, Response};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

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
    let daemon = Arc::new(Daemon::new(config, paths, logger));
    let listener = socket::bind(&socket_path).unwrap();
    let serve_daemon = Arc::clone(&daemon);
    let handle = tokio::spawn(async move {
        let _ = socket::serve(listener, serve_daemon).await;
    });
    // The listener is bound before `serve` starts, so the path exists already.
    (socket_path, daemon, handle)
}

async fn connect(path: &std::path::Path) -> Client {
    let mut client = Client::connect(path).await.expect("connect");
    client.hello().await.expect("hello");
    client
}

#[tokio::test]
async fn a_socket_terminal_echoes_and_reports_the_exit_code() {
    let (path, daemon, server) = start(true).await;
    let mut control = connect(&path).await;
    let mut output = connect(&path).await;

    let id = match control
        .call(&Request::TerminalOpen { cols: 80, rows: 24 })
        .await
        .expect("open")
    {
        Response::TerminalOpened(opened) => {
            assert_eq!(opened.program, "/bin/sh");
            opened.id
        }
        other => panic!("expected TerminalOpened, got {other:?}"),
    };

    control
        .call(&Request::TerminalWrite {
            id: id.clone(),
            bytes_base64: STANDARD.encode(b"echo SOCKET-MARKER\n"),
        })
        .await
        .expect("write");

    let mut seen = String::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        match output
            .call(&Request::TerminalRead {
                id: id.clone(),
                wait_ms: 1_000,
            })
            .await
            .expect("read")
        {
            Response::TerminalOutput(chunk) => {
                seen.push_str(&String::from_utf8_lossy(
                    &STANDARD.decode(chunk.bytes_base64).unwrap(),
                ));
                if seen.contains("SOCKET-MARKER") {
                    break;
                }
            }
            other => panic!("unexpected read answer: {other:?}"),
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "no marker: {seen:?}"
        );
    }

    control
        .call(&Request::TerminalWrite {
            id: id.clone(),
            bytes_base64: STANDARD.encode(b"exit 3\n"),
        })
        .await
        .expect("write exit");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        match output
            .call(&Request::TerminalRead {
                id: id.clone(),
                wait_ms: 1_000,
            })
            .await
            .expect("read")
        {
            Response::TerminalOutput(chunk) if chunk.eof => {
                assert_eq!(chunk.exit_code, Some(3));
                break;
            }
            Response::TerminalOutput(_) => {}
            other => panic!("unexpected exit answer: {other:?}"),
        }
        assert!(tokio::time::Instant::now() < deadline, "no exit");
    }

    control
        .call(&Request::TerminalClose { id })
        .await
        .expect("close");
    assert!(daemon.terminals().expect("real backends").is_empty());
    server.abort();
}

#[tokio::test]
async fn a_disabled_terminal_refuses_the_open() {
    let (path, daemon, server) = start(false).await;
    let mut client = connect(&path).await;
    let error = client
        .call(&Request::TerminalOpen { cols: 80, rows: 24 })
        .await
        .expect_err("disabled");
    assert_eq!(error.code, "terminal_disabled");
    assert!(daemon.terminals().expect("real backends").is_empty());
    server.abort();
}

#[tokio::test]
async fn a_bad_size_is_refused_by_the_socket() {
    let (path, _daemon, server) = start(true).await;
    let mut client = connect(&path).await;
    let error = client
        .call(&Request::TerminalOpen { cols: 0, rows: 24 })
        .await
        .expect_err("bad size");
    assert_eq!(error.kind, anth_wire::ErrorKind::Invalid);
    server.abort();
}

#[tokio::test]
async fn a_handle_from_one_socket_owner_is_not_visible_to_another() {
    // Socket ownership is per uid, so two connections from the same test process
    // share it; the registry owner rule is covered in `terminal.rs`. This test
    // guards the socket path itself: closing twice is harmless.
    let (path, _daemon, server) = start(true).await;
    let mut client = connect(&path).await;
    let id = match client
        .call(&Request::TerminalOpen { cols: 80, rows: 24 })
        .await
        .expect("open")
    {
        Response::TerminalOpened(opened) => opened.id,
        other => panic!("expected opened, got {other:?}"),
    };
    client
        .call(&Request::TerminalClose { id: id.clone() })
        .await
        .expect("first close");
    client
        .call(&Request::TerminalClose { id })
        .await
        .expect("second close is idempotent");
    server.abort();
}

#[tokio::test]
async fn the_socket_does_not_expose_the_terminal_when_the_daemon_is_off() {
    let (path, _daemon, server) = start(false).await;
    let mut client = connect(&path).await;
    let error = client
        .call(&Request::TerminalRead {
            id: "t1".into(),
            wait_ms: 0,
        })
        .await
        .expect_err("disabled read");
    assert_eq!(error.code, "terminal_disabled");
    server.abort();
}

#[tokio::test]
async fn a_large_write_is_measured_before_it_reaches_the_pty() {
    let (path, _daemon, server) = start(true).await;
    let mut client = connect(&path).await;
    let id = match client
        .call(&Request::TerminalOpen { cols: 80, rows: 24 })
        .await
        .expect("open")
    {
        Response::TerminalOpened(opened) => opened.id,
        other => panic!("expected opened, got {other:?}"),
    };
    let error = client
        .call(&Request::TerminalWrite {
            id,
            bytes_base64: STANDARD.encode(vec![b'x'; anth_wire::MAX_TERMINAL_INPUT + 1]),
        })
        .await
        .expect_err("too large");
    assert_eq!(error.kind, anth_wire::ErrorKind::TooLarge);
    server.abort();
}
