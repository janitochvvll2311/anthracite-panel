//! Interactive terminal for the CLI.
//!
//! `anth-panel terminal` opens one session over the same WebSocket endpoint the
//! browser uses. One task writes keystrokes and resizes; this task prints the
//! output and returns the shell's exit status.

use std::io::{IsTerminal, Write};
use std::path::Path;

use anth_client::Client;
use anth_wire::{DEFAULT_TERMINAL_SIZE, PROTOCOL_VERSION, TerminalServerMessage, TerminalSize};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

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
    size.validate()?;
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

/// One local event, so input and resize keep their order on the socket.
enum LocalEvent {
    Input(Vec<u8>),
    Resize(TerminalSize),
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
    let client = Client::unix(&paths.socket);
    match client.hello().await {
        Ok(hello) if hello.protocol_version == PROTOCOL_VERSION => {}
        Ok(hello) => {
            eprintln!(
                "[ERROR] the daemon speaks protocol {}, this CLI speaks {PROTOCOL_VERSION}",
                hello.protocol_version
            );
            return Err(1);
        }
        Err(error) => {
            eprintln!("[ERROR] {error}");
            return Err(1);
        }
    }

    let session = match client.terminal(size).await {
        Ok(session) => session,
        Err(error) => {
            eprintln!("[ERROR] {error}");
            return Err(1);
        }
    };

    let _raw = RawMode::enable();
    let (events, mut queue) = unbounded_channel();
    forward_input(events.clone());
    forward_resize(events);

    let (mut writer, mut reader) = session.split();
    let writer_task = tokio::spawn(async move {
        while let Some(event) = queue.recv().await {
            let sent = match event {
                LocalEvent::Input(bytes) => writer.input(&bytes).await,
                LocalEvent::Resize(size) => writer.resize(size).await,
            };
            if sent.is_err() {
                break;
            }
        }
    });

    let status = pump_output(&mut reader).await;
    writer_task.abort();

    if status == 0 {
        Ok(())
    } else if (0..=255).contains(&status) {
        Err(status as u8)
    } else {
        Err(1)
    }
}

/// Forwards local keystrokes until stdin closes.
fn forward_input(events: UnboundedSender<LocalEvent>) {
    tokio::spawn(async move {
        let mut stdin = tokio::io::stdin();
        let mut buffer = [0u8; 4096];
        loop {
            let read = match stdin.read(&mut buffer).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            if events
                .send(LocalEvent::Input(buffer[..read].to_vec()))
                .is_err()
            {
                break;
            }
        }
    });
}

/// Sends the local window size whenever the terminal reports a change.
fn forward_resize(events: UnboundedSender<LocalEvent>) {
    tokio::spawn(async move {
        let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::window_change())
        else {
            return;
        };
        while signal.recv().await.is_some() {
            let Some((cols, rows)) = terminal_size() else {
                continue;
            };
            let size = TerminalSize { cols, rows };
            if events.send(LocalEvent::Resize(size)).is_err() {
                break;
            }
        }
    });
}

/// Copies terminal output to stdout and returns the shell's exit status.
async fn pump_output(reader: &mut anth_client::TerminalReader) -> i32 {
    let mut stdout = std::io::stdout();
    loop {
        let message = match reader.next().await {
            Ok(Some(message)) => message,
            Ok(None) => {
                eprintln!("[ERROR] the daemon closed the terminal");
                return 1;
            }
            Err(error) => {
                eprintln!("[ERROR] {error}");
                return 1;
            }
        };
        match message {
            TerminalServerMessage::Ready { .. } | TerminalServerMessage::Pong => {}
            TerminalServerMessage::Output { data } => {
                let bytes = match STANDARD.decode(data.as_bytes()) {
                    Ok(bytes) => bytes,
                    Err(_) => {
                        eprintln!("[ERROR] the daemon sent invalid base64 output");
                        return 1;
                    }
                };
                if let Err(error) = stdout.write_all(&bytes).and_then(|()| stdout.flush()) {
                    if error.kind() != std::io::ErrorKind::BrokenPipe {
                        eprintln!("Failed to write terminal output: {error}");
                    }
                    return 1;
                }
            }
            TerminalServerMessage::Exit { code } => return code,
            TerminalServerMessage::Error { detail } => {
                eprintln!("[ERROR] {detail}");
                return 1;
            }
        }
    }
}
