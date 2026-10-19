//! Regression test for the terminal-exit hot loop.
//!
//! Once the last session exits, the connection must stop polling an exhausted
//! output stream and go back to waiting on the socket.

#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anth_panel::config::{Config, FilesConfig, ProcessConfig, TerminalConfig};
use anth_proto::{AnthClient, Request, Response, TerminalOpenArgs, TerminalSize};
use tokio::net::UnixStream;

fn unique_socket(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "anth-exit-{name}-{}-{nanos}.sock",
        std::process::id()
    ))
}

fn config_at(path: &Path) -> Config {
    Config {
        socket_path: path.to_string_lossy().into_owned(),
        terminal: TerminalConfig {
            shell: "/bin/sh".to_string(),
            token: String::new(),
            max_sessions: 1,
        },
        process: ProcessConfig::default(),
        files: FilesConfig::default(),
    }
}

/// User plus system CPU ticks for this process.
fn cpu_ticks() -> Option<u64> {
    let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
    let after_comm = stat.rsplit_once(')')?.1;
    let fields: Vec<&str> = after_comm.split_whitespace().collect();
    let utime: u64 = fields.get(11)?.parse().ok()?;
    let stime: u64 = fields.get(12)?.parse().ok()?;
    Some(utime + stime)
}

fn clock_ticks_per_second() -> u64 {
    let value = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    u64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .unwrap_or(100)
}

async fn start_daemon(config: Config, path: &Path) -> tokio::task::JoinHandle<()> {
    let _ = std::fs::remove_file(path);
    let handle = tokio::spawn(async move {
        let _ = anth_panel::daemon::serve(&config).await;
    });

    for _ in 0..200 {
        if path.exists() {
            return handle;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("daemon did not bind {}", path.display());
}

async fn next_response(client: &mut AnthClient) -> Response {
    tokio::time::timeout(Duration::from_secs(10), client.receive_response())
        .await
        .expect("timed out waiting for a daemon response")
        .expect("daemon closed the connection")
        .expect("malformed daemon response")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_exited_terminal_does_not_pin_the_connection() {
    let path = unique_socket("cpu");
    let daemon = start_daemon(config_at(&path), &path).await;

    let stream = UnixStream::connect(&path).await.unwrap();
    let mut client = AnthClient::new(stream);
    client
        .send_request(&Request::TerminalOpen(TerminalOpenArgs {
            size: Some(TerminalSize { cols: 80, rows: 24 }),
        }))
        .await
        .unwrap();

    let id = match next_response(&mut client).await {
        Response::TerminalOpened(opened) => opened.id,
        other => panic!("expected TerminalOpened, got {other:?}"),
    };

    client
        .send_request(&Request::TerminalInput {
            id,
            bytes: b"exit\n".to_vec(),
        })
        .await
        .unwrap();

    loop {
        match next_response(&mut client).await {
            Response::TerminalOutput { .. } => continue,
            Response::TerminalExited { status, .. } => {
                assert_eq!(status, 0);
                break;
            }
            Response::Error { detail } => panic!("terminal failed: {detail}"),
            other => panic!("unexpected response while exiting: {other:?}"),
        }
    }

    let before = cpu_ticks().expect("read /proc/self/stat");
    tokio::time::sleep(Duration::from_millis(400)).await;
    let after = cpu_ticks().expect("read /proc/self/stat");

    let cpu_seconds = (after - before) as f64 / clock_ticks_per_second() as f64;
    assert!(
        cpu_seconds < 0.15,
        "connection used {cpu_seconds:.3}s of CPU over 400ms after the terminal exited"
    );

    // The connection must remain usable after the last session is gone.
    client
        .send_request(&Request::Echo {
            message: "still here".to_string(),
        })
        .await
        .unwrap();
    match next_response(&mut client).await {
        Response::Ok { message } => assert_eq!(message, "still here"),
        other => panic!("expected Echo reply, got {other:?}"),
    }

    daemon.abort();
    let _ = std::fs::remove_file(&path);
}
