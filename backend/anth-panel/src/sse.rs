//! Server-sent events.
//!
//! [`SseStream`] turns any stream of items into SSE frames. It knows nothing
//! about host info, the daemon, or the router: a new streaming endpoint passes
//! its own event name and item stream and reuses the framing, the retry hint,
//! the keep-alive and the error event.
//!
//! An error ends the stream. Once the first frame is written the HTTP status can
//! no longer change, so a failure is reported as an `error` event and then the
//! response closes. That tells `EventSource` to reconnect instead of leaving it
//! reading a stream that will never recover.

use std::convert::Infallible;
use std::time::Duration;

use axum::response::sse::{Event, KeepAlive, Sse};
use futures::stream::{Stream, StreamExt};
use serde::Serialize;

/// How often an idle stream sends a comment to keep proxies from closing it.
pub const KEEP_ALIVE: Duration = Duration::from_secs(15);
/// Delay the browser should wait before reconnecting after a drop.
const RETRY: Duration = Duration::from_millis(3000);
/// SSE `id` field used for error events; regular frames are numbered from 1.
const ERROR_ID: &str = "error";

/// One SSE response carrying `T` values under a fixed event name.
pub struct SseStream {
    name: &'static str,
}

impl SseStream {
    pub fn new(name: &'static str) -> Self {
        Self { name }
    }

    /// Wraps `items` into a ready-to-return SSE response.
    ///
    /// The first error becomes an `error` event and ends the stream.
    pub fn stream<T, E, S>(self, items: S) -> Sse<impl Stream<Item = Result<Event, Infallible>>>
    where
        S: Stream<Item = Result<T, E>> + Send + 'static,
        T: Serialize + Send + 'static,
        E: ToString + Send + 'static,
    {
        let name = self.name;

        let frames = stop_after_error(items)
            .enumerate()
            .flat_map(move |(index, item)| {
                let mut frames = Vec::with_capacity(2);
                if index == 0 {
                    frames.push(Ok(Event::default().retry(RETRY)));
                }

                frames.push(Ok(match item {
                    Ok(value) => match Event::default().event(name).json_data(value) {
                        Ok(event) => event.id((index + 1).to_string()),
                        Err(e) => Event::default()
                            .event("error")
                            .id(ERROR_ID)
                            .data(e.into_inner().to_string()),
                    },
                    Err(e) => Event::default()
                        .event("error")
                        .id(ERROR_ID)
                        .data(e.to_string()),
                }));

                futures::stream::iter(frames)
            });

        Sse::new(frames).keep_alive(KeepAlive::new().interval(KEEP_ALIVE))
    }
}

/// Yields items until the first error, inclusive, then ends.
///
/// Vectors of frames cannot express "emit this and stop", and `scan` would poll
/// the source once more after the error — which is exactly the retry loop this
/// exists to prevent. Stopping here, before the mapping, guarantees the source
/// is never polled again once it has failed.
fn stop_after_error<S, T, E>(items: S) -> impl Stream<Item = Result<T, E>> + Send
where
    S: Stream<Item = Result<T, E>> + Send + 'static,
    T: Send + 'static,
    E: Send + 'static,
{
    futures::stream::unfold((Box::pin(items), false), |(mut items, done)| async move {
        if done {
            return None;
        }
        match items.next().await {
            Some(Ok(value)) => Some((Ok(value), (items, false))),
            Some(Err(e)) => Some((Err(e), (items, true))),
            None => None,
        }
    })
}
