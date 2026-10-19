//! Server-sent events.
//!
//! [`SseStream`] turns any stream of items into SSE frames. It knows nothing
//! about host info, the daemon, or the router: a new streaming endpoint passes
//! its own event name and item stream and reuses the framing, the retry hint,
//! the keep-alive and the error event.

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
    retry: bool,
}

impl SseStream {
    pub fn new(name: &'static str) -> Self {
        Self { name, retry: true }
    }

    /// Wraps `items` into a ready-to-return SSE response.
    ///
    /// Errors are sent as an `error` event and then end the stream, since the
    /// HTTP status can no longer change once the first frame is written.
    pub fn stream<T, E, S>(self, items: S) -> Sse<impl Stream<Item = Result<Event, Infallible>>>
    where
        S: Stream<Item = Result<T, E>> + Send + 'static,
        T: Serialize + Send + 'static,
        E: ToString + Send + 'static,
    {
        let SseStream { name, retry } = self;

        let frames = items.enumerate().flat_map(move |(index, item)| {
            let event = match item {
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
            };

            let mut frames = Vec::with_capacity(2);
            if retry && index == 0 {
                frames.push(Ok(Event::default().retry(RETRY)));
            }
            frames.push(Ok(event));
            futures::stream::iter(frames)
        });

        Sse::new(frames).keep_alive(KeepAlive::new().interval(KEEP_ALIVE))
    }
}
