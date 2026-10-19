//! Interactive terminal for the CLI.
//!
//! `anth-panel terminal` opens a session through the daemon and connects the
//! local terminal to it: stdin bytes go out as `TerminalInput`, daemon output
//! is written to stdout, and window changes resize the remote PTY. The web
//! terminal already speaks this protocol; this command drives it from a TTY.

use std::io::{IsTerminal, Write};
use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;

use anth_proto::{AnthReader, AnthWriter, Request, Response, TerminalOpenArgs, TerminalSize};
use tokio::io::AsyncReadExt;
use tokio::net::UnixStream;
use tokio::sync::Mutex;

use crate::cli::TerminalArgs;

/// Size used when the local terminal reports none.
const DEFAULT_COLS: u16 = 80;
const DEFAULT_ROWS: u16 = 24;

/// The size a session should start with: explicit flags first, then the local
/// terminal's size, then the default.
pub fn resolve_size(args: &TerminalArgs) -> Result<TerminalSize, String> {
    resolve_size_from(args, terminal_size())
}

/// [`resolve_size`] with the detected size supplied, so tests are deterministic.
pub fn resolve_size_from(
    args: &TerminalArgs,
    detected: Option<(u16, u16)>,
) -> Result<TerminalSize, String> {
    let cols = args
        .cols
        .or(detected.map(|(c, _)| c))
        .unwrap_or(DEFAULT_COLS);
    let rows = args
        .rows
        .or(detected.map(|(_, r)| r))
        .unwrap_or(DEFAULT_ROWS);
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

/// The local terminal's size, from the `TIOCGWINSZ` ioctl on stdout.
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

/// Puts the local terminal into raw mode for the life of the guard.
///
/// Raw mode disables echo, line buffering and signal generation, so keystrokes
/// and Ctrl-C travel to the remote shell instead of being interpreted locally.
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

/// Connects to the daemon and runs an interactive terminal session.
pub async fn run(socket_path: &Path, args: &TerminalArgs) -> ExitCode {
    let size = match resolve_size(args) {
        Ok(size) => size,
        Err(e) => {
            eprintln!("[ERROR] {e}");
            return ExitCode::FAILURE;
        }
    };

    let stream = match UnixStream::connect(socket_path).await {
        Ok(stream) => stream,
        Err(e) => {
            eprintln!("Failed to connect to socket: {e}");
            return ExitCode::FAILURE;
        }
    };

    let (read_half, write_half) = stream.into_split();
    let mut reader = AnthReader::from_read_half(read_half);
    let writer = Arc::new(Mutex::new(AnthWriter::from_write_half(write_half)));

    // Open the session before touching the local terminal, so a refused open
    // never leaves the TTY in raw mode.
    {
        let mut writer = writer.lock().await;
        if let Err(e) = writer
            .send_request(&Request::TerminalOpen(TerminalOpenArgs {
                size: Some(size),
            }))
            .await
        {
            eprintln!("Failed to send terminal request: {e}");
            return ExitCode::FAILURE;
        }
    }

    let id = match reader.receive_response().await {
        Some(Ok(Response::TerminalOpened(opened))) => opened.id,
        Some(Ok(Response::Error { detail })) => {
            eprintln!("[ERROR] {detail}");
            return ExitCode::FAILURE;
        }
        Some(Ok(other)) => {
            eprintln!("[ERROR] unexpected response to terminal open: {other:?}");
            return ExitCode::FAILURE;
        }
        Some(Err(e)) => {
            eprintln!("[ERROR] {e}");
            return ExitCode::FAILURE;
        }
        None => {
            eprintln!("[ERROR] daemon closed the connection before opening a terminal");
            return ExitCode::FAILURE;
        }
    };

    let _raw = RawMode::enable();

    // stdin -> TerminalInput. The daemon acknowledges success silently, so this
    // task only writes and never reads; output is handled below.
    {
        let writer = Arc::clone(&writer);
        tokio::spawn(async move {
            let mut input = tokio::io::stdin();
            let mut buffer = [0u8; 4096];
            loop {
                let read = match AsyncReadExt::read(&mut input, &mut buffer).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                let mut writer = writer.lock().await;
                if writer
                    .send_request(&Request::TerminalInput {
                        id,
                        bytes: buffer[..read].to_vec(),
                    })
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });
    }

    // SIGWINCH -> TerminalResize.
    if let Ok(mut signal) =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::window_change())
    {
        let writer = Arc::clone(&writer);
        tokio::spawn(async move {
            while signal.recv().await.is_some() {
                if let Some((cols, rows)) = terminal_size() {
                    let mut writer = writer.lock().await;
                    let _ = writer
                        .send_request(&Request::TerminalResize {
                            id,
                            size: TerminalSize { cols, rows },
                        })
                        .await;
                }
            }
        });
    }

    // The session ends exactly once; everything else on this half is output.
    let mut status = -1;
    let mut stdout = std::io::stdout();
    loop {
        match reader.receive_response().await {
            Some(Ok(Response::TerminalOutput { bytes, .. })) => {
                if let Err(e) = stdout.write_all(&bytes).and_then(|()| stdout.flush()) {
                    if e.kind() != std::io::ErrorKind::BrokenPipe {
                        eprintln!("Failed to write terminal output: {e}");
                    }
                    break;
                }
            }
            Some(Ok(Response::TerminalExited { status: code, .. })) => {
                status = code;
                break;
            }
            Some(Ok(Response::Error { detail })) => {
                eprintln!("[ERROR] {detail}");
                break;
            }
            Some(Ok(other)) => {
                eprintln!("[ERROR] unexpected terminal response: {other:?}");
                break;
            }
            Some(Err(e)) => {
                eprintln!("Failed to receive response: {e}");
                break;
            }
            None => break,
        }
    }

    // Best effort: tell the daemon to drop the session, then restore the TTY.
    {
        let mut writer = writer.lock().await;
        let _ = writer.send_request(&Request::TerminalClose { id }).await;
    }

    if status == 0 {
        ExitCode::SUCCESS
    } else if (0..=255).contains(&status) {
        ExitCode::from(status as u8)
    } else {
        ExitCode::FAILURE
    }
}
