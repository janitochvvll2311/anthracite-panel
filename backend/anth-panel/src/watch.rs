//! Repeating one request until interrupted.
//!
//! `host-info --watch` sends the same `Request::HostInfo` the one-shot command
//! sends, once per interval. The loop lives here, on the client, for the reason
//! `file read --follow` gives: the protocol still carries one request and one
//! answer, `dispatch` keeps its shape, and a stream would be a second way to
//! deliver a value the wire already delivers. A daemon that sent frames on its
//! own would also have to answer "who stops it", and every answer to that is
//! state the rewrite does not need.
//!
//! One connection carries every round, so watching is one handshake rather than
//! one per tick. A dropped connection is therefore the end of the watch rather
//! than a silent gap: the loop reports it and exits.

use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use anth_wire::{Request, Response};

use crate::render;
use crate::socket::Client;

/// Writes one frame per round.
///
/// A terminal gets the previous frame overwritten in place, because a table
/// that scrolls once every two seconds is unreadable and the cursor is the one
/// thing a terminal offers for this. A pipe gets a blank line between frames
/// instead: escape sequences in a redirected log are noise, and `| head` should
/// read as text.
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

    /// Returns `false` when the reader went away, which is not an error: `head`
    /// closing the pipe is the caller's decision, not a failure of the watch.
    pub fn write_frame(&mut self, text: &str) -> io::Result<()> {
        if self.written > 0 {
            if self.redraw {
                // Back to the first line of the previous frame, then erase from
                // there: the new frame may be shorter (swap disappearing, a
                // core going away) and leftovers would be lies.
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

/// Prints a host snapshot now and then, until interrupted.
///
/// Returns success on `Ctrl-C` and on a closed stdout: an operator who stops a
/// watch, or pipes it through `head`, asked for that. A refused request ends the
/// watch, because a daemon that answered one `Error` is not a daemon this loop
/// should keep asking.
pub async fn host_info(socket: &Path, period: Duration) -> ExitCode {
    watch_into(
        socket,
        period,
        Frames::new(io::stdout(), io::stdout().is_terminal()),
    )
    .await
}

/// The watch itself, writing into whatever the caller hands it.
///
/// Split from [`host_info`] so a test can drive the real loop — a real socket, a
/// real request, the real renderer — and observe the bytes, instead of asserting
/// that a function was called. The only thing the test supplies is the writer,
/// and the interesting writer is one that stops accepting frames, which is what
/// `| head` is.
pub async fn watch_into<W: Write>(
    socket: &Path,
    period: Duration,
    mut frames: Frames<W>,
) -> ExitCode {
    let mut client = match Client::connect(socket).await {
        Ok(client) => client,
        Err(error) => {
            eprintln!("anth-panel: {}", error.message);
            return ExitCode::from(error.kind.exit_code());
        }
    };

    // The first tick of an interval is immediate, so the command draws a frame
    // at once instead of going quiet for one period.
    let mut ticker = tokio::time::interval(period);
    // A slow answer must not be followed by a burst of catch-up frames: with the
    // default behaviour, missing two ticks delivers them back to back.
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => return ExitCode::SUCCESS,
            _ = ticker.tick() => {}
        }

        let response = match client.call(&Request::HostInfo).await {
            Ok(response) => response,
            Err(error) => {
                eprintln!("anth-panel: {}", error.message);
                return ExitCode::from(error.kind.exit_code());
            }
        };

        match response {
            Response::HostInfo(info) => {
                if let Err(error) = frames.write_frame(&render::render_host_info(&info)) {
                    if error.kind() == io::ErrorKind::BrokenPipe {
                        return ExitCode::SUCCESS;
                    }
                    eprintln!("anth-panel: cannot write to stdout: {error}");
                    return ExitCode::FAILURE;
                }
            }
            Response::Error(error) => {
                eprintln!("anth-panel: {}", error.message);
                return ExitCode::from(error.kind.exit_code());
            }
            other => {
                eprintln!("anth-panel: unexpected answer to a host snapshot: {other:?}");
                return ExitCode::FAILURE;
            }
        }
    }
}
