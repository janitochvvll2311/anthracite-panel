//! A minimal server-sent-events reader.
//!
//! The daemon streams snapshots as `data:` lines. This reader ignores comments
//! (the keep-alive frames), `event:` names and `id:` values, because the caller
//! already knows which stream it opened.

use std::marker::PhantomData;

use anth_wire::WireError;
use serde::de::DeserializeOwned;

use crate::transport::Download;

/// One SSE stream decoded into DTOs.
pub struct EventStream<T> {
    body: Download,
    buffer: Vec<u8>,
    finished: bool,
    marker: PhantomData<fn() -> T>,
}

impl<T> EventStream<T>
where
    T: DeserializeOwned,
{
    pub(crate) fn new(body: Download) -> Self {
        Self {
            body,
            buffer: Vec::new(),
            finished: false,
            marker: PhantomData,
        }
    }

    /// Reads the next event, or `None` when the stream ends.
    pub async fn next(&mut self) -> Result<Option<T>, WireError> {
        loop {
            if let Some(bytes) = self.take_event() {
                match parse_data(&bytes)? {
                    Some(data) => {
                        return serde_json::from_str(&data).map(Some).map_err(|error| {
                            WireError::internal(format!("the stream sent invalid JSON: {error}"))
                        });
                    }
                    None => continue,
                }
            }
            if self.finished {
                return Ok(None);
            }
            match self.body.read().await? {
                Some(chunk) => {
                    self.buffer.extend_from_slice(&chunk);
                    // CR is only a line separator in SSE; dropping it early also
                    // handles a CRLF split across two chunks.
                    self.buffer.retain(|byte| *byte != b'\r');
                }
                None => self.finished = true,
            }
        }
    }

    fn take_event(&mut self) -> Option<Vec<u8>> {
        let end = self.buffer.windows(2).position(|pair| pair == b"\n\n")?;
        let event = self.buffer[..end].to_vec();
        self.buffer.drain(..end + 2);
        Some(event)
    }
}

fn parse_data(event: &[u8]) -> Result<Option<String>, WireError> {
    let text = std::str::from_utf8(event)
        .map_err(|error| WireError::internal(format!("the stream is not UTF-8: {error}")))?;
    let mut data: Vec<&str> = Vec::new();
    for line in text.split('\n') {
        if let Some(rest) = line.strip_prefix("data:") {
            data.push(rest.strip_prefix(' ').unwrap_or(rest));
        }
    }
    if data.is_empty() {
        Ok(None)
    } else {
        Ok(Some(data.join("\n")))
    }
}
