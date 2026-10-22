//! Repeating a client request until interrupted.
//!
//! The loop is request-agnostic: it sends whatever [`Request`] it is handed and
//! prints whatever the caller's renderer produces. A subcommand opts into
//! streaming by exposing an interval and passing it here, so `host-info` is the
//! only current user but not a special case.

use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use anth_proto::{AnthClient, Request, Response};
use futures::StreamExt;
use tokio::net::UnixStream;
use tokio::sync::Mutex;

use crate::poll;

pub const DEFAULT_INTERVAL: Duration = Duration::from_secs(2);
pub const MIN_INTERVAL: Duration = Duration::from_millis(10);
const MAX_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// Parses a human interval such as `500ms`, `2s`, `1.5s`, `1m` or `1h`.
pub fn parse_interval(raw: &str) -> Result<Duration, String> {
    let raw = raw.trim();
    let (digits, unit_millis) = if let Some(rest) = raw.strip_suffix("ms") {
        (rest, 1.0)
    } else if let Some(rest) = raw.strip_suffix('s') {
        (rest, 1_000.0)
    } else if let Some(rest) = raw.strip_suffix('m') {
        (rest, 60_000.0)
    } else if let Some(rest) = raw.strip_suffix('h') {
        (rest, 3_600_000.0)
    } else {
        return Err(format!(
            "invalid interval `{raw}`: add a unit suffix (ms, s, m or h), e.g. 500ms, 2s, 1m"
        ));
    };

    let value: f64 = digits.trim().parse().map_err(|_| {
        format!(
            "invalid interval `{raw}`: `{}` is not a number",
            digits.trim()
        )
    })?;
    if !value.is_finite() || value <= 0.0 {
        return Err(format!(
            "invalid interval `{raw}`: must be greater than zero"
        ));
    }

    let millis = value * unit_millis;
    let low = MIN_INTERVAL.as_millis() as f64;
    if millis < low {
        return Err(format!(
            "interval too small: `{raw}` is below the {low}ms minimum"
        ));
    }
    if millis > MAX_INTERVAL.as_millis() as f64 {
        return Err(format!(
            "interval too large: `{raw}` exceeds the {}-hour maximum",
            MAX_INTERVAL.as_secs() / 3600
        ));
    }

    Ok(Duration::from_millis(millis.round() as u64))
}

/// Writes one frame per read. On a terminal each frame overwrites the previous
/// one; when redirected, frames are separated by a blank line so a log stays
/// readable.
pub struct Frames<W: Write> {
    writer: W,
    redraw: bool,
    written: usize,
    height: usize,
}

impl<W: Write> Frames<W> {
    pub fn new(writer: W, redraw: bool) -> Self {
        Self {
            writer,
            redraw,
            written: 0,
            height: 0,
        }
    }

    pub fn write_frame(&mut self, text: &str) -> io::Result<()> {
        if self.written > 0 {
            if self.redraw {
                // Move to the first line of the previous frame and erase from there.
                write!(self.writer, "\x1b[{}A\x1b[J", self.height)?;
            } else {
                writeln!(self.writer)?;
            }
        }
        writeln!(self.writer, "{text}")?;
        self.writer.flush()?;

        self.written += 1;
        self.height = text.matches('\n').count() + 1;
        Ok(())
    }
}

/// Reads once when `period` is `None`, or repeatedly until interrupted.
pub async fn follow<F>(
    socket_path: &Path,
    request: &Request,
    period: Option<Duration>,
    mut render: F,
) -> ExitCode
where
    F: FnMut(&Response) -> String,
{
    let socket = match UnixStream::connect(socket_path).await {
        Ok(stream) => stream,
        Err(e) => {
            eprintln!("Failed to connect to socket: {e}");
            return ExitCode::FAILURE;
        }
    };

    // One connection for the whole stream, shared by successive reads.
    let client = Arc::new(Mutex::new(AnthClient::new(socket)));
    let request = Arc::new(request.clone());
    let mut items = Box::pin(poll::maybe_stream(period, move || {
        let client = Arc::clone(&client);
        let request = Arc::clone(&request);
        async move {
            let mut client = client.lock().await;
            read_once(&mut client, &request).await
        }
    }));

    let mut frames = Frames::new(io::stdout(), io::stdout().is_terminal());

    loop {
        let item = tokio::select! {
            _ = tokio::signal::ctrl_c() => return ExitCode::SUCCESS,
            item = items.next() => item,
        };

        match item {
            Some(Ok(Response::Error { detail })) => {
                eprintln!("[ERROR] {detail}");
                return ExitCode::FAILURE;
            }
            Some(Ok(Response::FileError(error))) => {
                eprintln!("[ERROR] {}", error.detail);
                return ExitCode::FAILURE;
            }
            Some(Ok(Response::NetworkError(error))) => {
                eprintln!("[ERROR] {}", error.detail);
                return ExitCode::FAILURE;
            }
            Some(Ok(Response::FirewallError(error))) => {
                eprintln!("[ERROR] {}", error.detail);
                return ExitCode::FAILURE;
            }
            Some(Ok(Response::VolumeError(error))) => {
                eprintln!("[ERROR] {}", error.detail);
                return ExitCode::FAILURE;
            }
            Some(Ok(Response::PartitionError(error))) => {
                eprintln!("[ERROR] {}", error.detail);
                return ExitCode::FAILURE;
            }
            Some(Ok(Response::ProcessSpawnError(error))) => {
                eprintln!("[ERROR] {}", error.detail);
                return ExitCode::FAILURE;
            }
            Some(Ok(response)) => match frames.write_frame(&render(&response)) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::BrokenPipe => return ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("Failed to write output: {e}");
                    return ExitCode::FAILURE;
                }
            },
            Some(Err(e)) => {
                eprintln!("{e}");
                return ExitCode::FAILURE;
            }
            None => return ExitCode::SUCCESS,
        }
    }
}

pub async fn read_once(client: &mut AnthClient, request: &Request) -> Result<Response, String> {
    client
        .send_request(request)
        .await
        .map_err(|e| format!("Failed to send request: {e}"))?;

    match client.receive_response().await {
        Some(Ok(response)) => Ok(response),
        Some(Err(e)) => Err(format!("Failed to receive response: {e}")),
        None => Err("Stream closed while waiting for response".to_string()),
    }
}
