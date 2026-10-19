//! Repeating a client request until interrupted.
//!
//! The loop is request-agnostic: it sends whatever [`Request`] it is handed and
//! prints whatever the caller's renderer produces. A subcommand opts into
//! streaming by exposing an interval and passing it here, so `host-info` is the
//! only current user but not a special case.

use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use anth_proto::{AnthClient, Request, Response};
use tokio::net::UnixStream;
use tokio::time::{MissedTickBehavior, interval};

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
///
/// The wait between reads yields to the runtime instead of spinning, and a read
/// that overruns the interval does not trigger a catch-up burst.
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

    let mut client = AnthClient::new(socket);
    let mut frames = Frames::new(io::stdout(), io::stdout().is_terminal());

    let mut ticker = period.map(|period| {
        let mut ticker = interval(period);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        ticker
    });
    if let Some(ticker) = ticker.as_mut() {
        // A Tokio interval fires its first tick immediately; consuming it makes
        // the first wait a full period after the first read.
        ticker.tick().await;
    }

    loop {
        if let Err(e) = client.send_request(request).await {
            eprintln!("Failed to send request: {e}");
            return ExitCode::FAILURE;
        }

        let response = match client.receive_response().await {
            Some(Ok(response)) => response,
            Some(Err(e)) => {
                eprintln!("Failed to receive response: {e}");
                return ExitCode::FAILURE;
            }
            None => {
                eprintln!("Stream closed while waiting for response");
                return ExitCode::FAILURE;
            }
        };

        match &response {
            Response::Error { detail } => {
                eprintln!("[ERROR] {detail}");
                return ExitCode::FAILURE;
            }
            other => match frames.write_frame(&render(other)) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::BrokenPipe => return ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("Failed to write output: {e}");
                    return ExitCode::FAILURE;
                }
            },
        }

        let Some(ticker) = ticker.as_mut() else {
            return ExitCode::SUCCESS;
        };

        tokio::select! {
            _ = ticker.tick() => {}
            _ = tokio::signal::ctrl_c() => return ExitCode::SUCCESS,
        }
    }
}
