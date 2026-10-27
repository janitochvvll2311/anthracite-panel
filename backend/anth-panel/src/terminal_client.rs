//! Interactive terminal for the CLI.
//!
//! `anth-panel terminal` opens one session through the daemon and connects the
//! local terminal to it. The socket is strictly request/response, so the CLI
//! holds two connections: one for input/resize/close and one for the long-poll
//! output. Both are the same peer uid, which is the owner the daemon assigns.

use std::io::{IsTerminal, Write};
use std::path::Path;

use std::sync::Arc;

use anth_wire::{DEFAULT_TERMINAL_SIZE, MAX_TERMINAL_WAIT_MS, Request, Response, TerminalSize};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use tokio::io::AsyncReadExt;
use tokio::sync::Mutex;

use crate::client::Client;

/// Resolves the size from flags, then the local TTY, then the shared default.
fn resolve_size(cols: Option<u16>, rows: Option<u16>) -> Result<TerminalSize, String> {
    let detected = terminal_size();
    let cols = cols
        .or(detected.map(|(cols, _)| cols))
        .unwrap_or(DEFAULT_TERMINAL_SIZE.cols);
    let rows = rows
        .or(detected.map(|(_, rows)| rows))
        .unwrap_or(DEFAULT_TERMINAL_SIZE.rows);
    let size = TerminalSize { cols, rows };
    if !size.is_sane() {
        return Err(format!(
            "terminal size {cols}x{rows} is outside {}x{}..{}x{}",
            TerminalSize::MIN.cols,
            TerminalSize::MIN.rows,
            TerminalSize::MAX.cols,
            TerminalSize::MAX.rows
        ));
    }
    Ok(size)
}

fn terminal_size() -> Option<(u16, u16)> {
    let mut window = std::mem::MaybeUninit::<libc::winsize>::uninit();
    if unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, window.as_mut_ptr()) } != 0 {
        return None;
    }
    let window = unsafe { window.assume_init() };
    if window.ws_col == 0 || window.ws_row == 0 {
        return None;
    }
    Some((window.ws_col, window.ws_row))
}

/// Raw mode for the life of the guard: keystrokes and signals go to the PTY.
struct RawMode {
    original: libc::termios,
}

impl RawMode {
    fn enable() -> Option<Self> {
        if !std::io::stdin().is_terminal() {
            return None;
        }
        let mut original = std::mem::MaybeUninit::<libc::termios>::uninit();
        if unsafe { libc::tcgetattr(libc::STDIN_FILENO, original.as_mut_ptr()) } != 0 {
            return None;
        }
        let original = unsafe { original.assume_init() };
        let mut raw = original;
        raw.c_iflag &= !(libc::IGNBRK
            | libc::BRKINT
            | libc::PARMRK
            | libc::ISTRIP
            | libc::INLCR
            | libc::IGNCR
            | libc::ICRNL
            | libc::IXON);
        raw.c_oflag &= !(libc::OPOST);
        raw.c_lflag &= !(libc::ECHO | libc::ECHONL | libc::ICANON | libc::ISIG | libc::IEXTEN);
        raw.c_cflag &= !(libc::CSIZE | libc::PARENB);
        raw.c_cflag |= libc::CS8;
        raw.c_cc[libc::VMIN] = 1;
        raw.c_cc[libc::VTIME] = 0;
        if unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw) } != 0 {
            return None;
        }
        Some(Self { original })
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        unsafe {
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &self.original);
        }
    }
}

pub async fn run(socket_path: &Path, cols: Option<u16>, rows: Option<u16>) -> Result<(), u8> {
    let size = match resolve_size(cols, rows) {
        Ok(size) => size,
        Err(error) => {
            eprintln!("[ERROR] {error}");
            return Err(1);
        }
    };

    let config = crate::config::Config::load_or_default(socket_path);
    let paths = match config.paths(socket_path) {
        Ok(paths) => paths,
        Err(error) => {
            eprintln!("[ERROR] {error}");
            return Err(1);
        }
    };
    let socket = paths.socket.as_path();

    let mut control = match Client::connect(socket).await {
        Ok(mut client) => {
            if let Err(error) = client.hello().await {
                eprintln!("[ERROR] {error}");
                return Err(1);
            }
            client
        }
        Err(error) => {
            eprintln!("Failed to connect to the daemon: {error}");
            return Err(1);
        }
    };
    let mut output = match Client::connect(socket).await {
        Ok(mut client) => {
            if let Err(error) = client.hello().await {
                eprintln!("[ERROR] {error}");
                return Err(1);
            }
            client
        }
        Err(error) => {
            eprintln!("Failed to connect to the daemon: {error}");
            return Err(1);
        }
    };

    let opened = match control
        .call(&Request::TerminalOpen {
            cols: size.cols,
            rows: size.rows,
        })
        .await
    {
        Ok(Response::TerminalOpened(opened)) => opened,
        Ok(Response::Error(error)) => {
            eprintln!("[ERROR] {error}");
            return Err(1);
        }
        Ok(other) => {
            eprintln!("[ERROR] unexpected terminal open response: {other:?}");
            return Err(1);
        }
        Err(error) => {
            eprintln!("[ERROR] {error}");
            return Err(1);
        }
    };

    let id = opened.id;
    let _raw = RawMode::enable();
    let control = Arc::new(Mutex::new(control));

    {
        let control = Arc::clone(&control);
        let id = id.clone();
        tokio::spawn(async move {
            let mut stdin = tokio::io::stdin();
            let mut buffer = [0u8; 4096];
            loop {
                let read = match stdin.read(&mut buffer).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                let mut client = control.lock().await;
                if client
                    .call(&Request::TerminalWrite {
                        id: id.clone(),
                        bytes_base64: STANDARD.encode(&buffer[..read]),
                    })
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });
    }

    if let Ok(mut signal) =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::window_change())
    {
        let control = Arc::clone(&control);
        let id = id.clone();
        tokio::spawn(async move {
            while signal.recv().await.is_some() {
                if let Some((cols, rows)) = terminal_size() {
                    let mut client = control.lock().await;
                    let _ = client
                        .call(&Request::TerminalResize {
                            id: id.clone(),
                            cols,
                            rows,
                        })
                        .await;
                }
            }
        });
    }

    let mut status = -1;
    let mut stdout = std::io::stdout();
    loop {
        let response = match output
            .call(&Request::TerminalRead {
                id: id.clone(),
                wait_ms: MAX_TERMINAL_WAIT_MS,
            })
            .await
        {
            Ok(response) => response,
            Err(error) => {
                eprintln!("[ERROR] {error}");
                break;
            }
        };
        match response {
            Response::TerminalOutput(chunk) => {
                if !chunk.bytes_base64.is_empty() {
                    let bytes = STANDARD.decode(&chunk.bytes_base64);
                    if let Ok(bytes) = bytes
                        && let Err(error) = stdout.write_all(&bytes).and_then(|()| stdout.flush())
                    {
                        if error.kind() != std::io::ErrorKind::BrokenPipe {
                            eprintln!("Failed to write terminal output: {error}");
                        }
                        break;
                    }
                }
                if chunk.eof {
                    status = chunk.exit_code.unwrap_or(-1);
                    break;
                }
            }
            Response::Error(error) => {
                eprintln!("[ERROR] {error}");
                break;
            }
            other => {
                eprintln!("[ERROR] unexpected terminal response: {other:?}");
                break;
            }
        }
    }

    {
        let mut client = control.lock().await;
        let _ = client.call(&Request::TerminalClose { id }).await;
    }

    if status == 0 {
        Ok(())
    } else if (0..=255).contains(&status) {
        Err(status as u8)
    } else {
        Err(1)
    }
}
