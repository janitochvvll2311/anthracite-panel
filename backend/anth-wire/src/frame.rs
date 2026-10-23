//! Length-prefixed framing over any async byte stream.
//!
//! Frame layout: `u32` big-endian payload length, then that many bytes of JSON.
//! A frame carries exactly one message, so a payload may contain newlines, and
//! its size is limited by [`MAX_FRAME_LEN`] rather than by a line rule.
//!
//! The limit is a transport guard: it stops a peer from making the daemon
//! buffer without bound. It is not a data-loss policy — a message that would
//! exceed it is rejected with [`ErrorKind::TooLarge`], and the caller decides
//! what to do. That distinction is the whole point of replacing v0.1.0's
//! newline framing, where exceeding the cap silently shortened the payload.

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::error::{ErrorKind, WireError};

/// Largest payload a frame may carry: 64 MiB.
///
/// Comfortably above any single snapshot: v0.1.0's whole design strained to fit
/// 1 MiB, and the largest real payload it produced was a ruleset dump. Lists
/// that can exceed this paginate instead of truncating.
pub const MAX_FRAME_LEN: usize = 64 * 1024 * 1024;

/// Reads one frame, rejecting anything above [`MAX_FRAME_LEN`].
///
/// Returns `Ok(None)` at a clean end of stream (the peer closed before sending a
/// length), which is how a daemon notices a disconnected client.
pub async fn read_frame<R>(reader: &mut R) -> Result<Option<Vec<u8>>, WireError>
where
    R: AsyncRead + Unpin,
{
    read_frame_limited(reader, MAX_FRAME_LEN).await
}

/// Reads one frame with an explicit limit.
///
/// Exposed so a test can prove the limit is enforced without allocating 64 MiB,
/// and so a long-lived stream (terminal output) can pick a tighter bound.
pub async fn read_frame_limited<R>(reader: &mut R, max: usize) -> Result<Option<Vec<u8>>, WireError>
where
    R: AsyncRead + Unpin,
{
    let mut header = [0u8; 4];
    match reader.read_exact(&mut header).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(WireError::internal(format!("reading frame header: {e}"))),
    }

    let len = u32::from_be_bytes(header) as usize;
    if len > max {
        return Err(WireError::new(
            ErrorKind::TooLarge,
            format!("frame of {len} bytes exceeds the {max}-byte limit"),
        ));
    }

    let mut payload = vec![0u8; len];
    reader
        .read_exact(&mut payload)
        .await
        .map_err(|e| WireError::internal(format!("reading {len} byte payload: {e}")))?;
    Ok(Some(payload))
}

/// Writes one frame.
pub async fn write_frame<W>(writer: &mut W, payload: &[u8]) -> Result<(), WireError>
where
    W: AsyncWrite + Unpin,
{
    write_frame_limited(writer, payload, MAX_FRAME_LEN).await
}

/// Writes one frame with an explicit limit.
///
/// The length is checked before anything is written, so a rejected payload never
/// leaves a half-sent frame on the stream.
pub async fn write_frame_limited<W>(
    writer: &mut W,
    payload: &[u8],
    max: usize,
) -> Result<(), WireError>
where
    W: AsyncWrite + Unpin,
{
    if payload.len() > max {
        return Err(WireError::new(
            ErrorKind::TooLarge,
            format!(
                "frame of {} bytes exceeds the {max}-byte limit",
                payload.len()
            ),
        ));
    }

    let len = u32::try_from(payload.len())
        .map_err(|_| WireError::internal("payload length does not fit in u32"))?;
    writer
        .write_all(&len.to_be_bytes())
        .await
        .map_err(|e| WireError::internal(format!("writing frame header: {e}")))?;
    writer
        .write_all(payload)
        .await
        .map_err(|e| WireError::internal(format!("writing frame payload: {e}")))?;
    writer
        .flush()
        .await
        .map_err(|e| WireError::internal(format!("flushing frame: {e}")))
}
