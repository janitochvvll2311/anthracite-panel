//! Repeating an async read on a fixed period.
//!
//! This module owns *when* a read happens and nothing else: no transport, no
//! formatting. Both the CLI (`--watch`) and the HTTP SSE endpoint build on it,
//! and a future command can do the same without copying the timer logic.

use std::future::Future;
use std::time::Duration;

use futures::stream::{self, Stream, StreamExt};
use tokio::time::{Interval, MissedTickBehavior, interval};

/// Polls `read` forever, waiting `period` between completed reads.
///
/// The first read happens immediately. Waits use one Tokio timer for the whole
/// stream, so the cadence stays fixed, the task yields instead of spinning, and
/// `Delay` keeps a slow read from causing a catch-up burst. Dropping the stream
/// cancels the pending wait with it.
///
/// Errors are yielded as `Err(E)` without ending the stream, leaving that
/// decision to the caller.
pub fn stream<T, E, F, Fut>(period: Duration, read: F) -> impl Stream<Item = Result<T, E>> + Send
where
    F: FnMut() -> Fut + Send + 'static,
    Fut: Future<Output = Result<T, E>> + Send,
    T: Send + 'static,
    E: Send + 'static,
{
    stream::unfold(
        (new_ticker(period), read),
        |(mut ticker, mut read)| async move {
            let item = read().await;
            // `interval` fires immediately; the first tick is consumed after the
            // first read so that wait is a full period.
            ticker.tick().await;
            Some((item, (ticker, read)))
        },
    )
}

/// Polls `read` once when `period` is `None`, otherwise delegates to [`stream`].
pub fn maybe_stream<T, E, F, Fut>(
    period: Option<Duration>,
    mut read: F,
) -> impl Stream<Item = Result<T, E>> + Send
where
    F: FnMut() -> Fut + Send + 'static,
    Fut: Future<Output = Result<T, E>> + Send + 'static,
    T: Send + 'static,
    E: Send + 'static,
{
    match period {
        None => stream::once(async move { read().await }).boxed(),
        Some(period) => stream(period, read).boxed(),
    }
}

fn new_ticker(period: Duration) -> Interval {
    let mut ticker = interval(period);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    ticker
}
