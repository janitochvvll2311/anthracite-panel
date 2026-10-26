//! Length-prefixed framing over any async byte stream.
//!
//! A frame is a `u32` big-endian payload length followed by that many bytes of
//! JSON. The limit is a transport guard, not a data-loss policy: an oversized
//! message is rejected with [`ErrorKind::TooLarge`], never shortened.

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::error::{ErrorKind, WireError};

/// Largest payload a frame may carry: 64 MiB.
pub const MAX_FRAME_LEN: usize = 64 * 1024 * 1024;

/// Reads one frame, rejecting anything above [`MAX_FRAME_LEN`].
///
/// Returns `Ok(None)` at a clean end of stream before a header is read.
pub async fn read_frame<R>(reader: &mut R) -> Result<Option<Vec<u8>>, WireError>
where
    R: AsyncRead + Unpin,
{
    read_frame_limited(reader, MAX_FRAME_LEN).await
}

/// Reads one frame with an explicit limit.
pub async fn read_frame_limited<R>(reader: &mut R, max: usize) -> Result<Option<Vec<u8>>, WireError>
where
    R: AsyncRead + Unpin,
{
    let mut header = [0u8; 4];
    match reader.read_exact(&mut header).await {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => {
            return Err(WireError::internal(format!(
                "reading frame header: {error}"
            )));
        }
    }

    let len = u32::from_be_bytes(header) as usize;
    if len > max {
        return Err(WireError::new(
            ErrorKind::TooLarge,
            "frame_too_large",
            format!("frame of {len} bytes exceeds the {max}-byte limit"),
        ));
    }

    let mut payload = vec![0u8; len];
    reader
        .read_exact(&mut payload)
        .await
        .map_err(|error| WireError::internal(format!("reading {len}-byte payload: {error}")))?;
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
/// The length is checked before anything is written, so a rejected payload
/// never leaves a half-sent frame on the stream.
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
            "frame_too_large",
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
        .map_err(|error| WireError::internal(format!("writing frame header: {error}")))?;
    writer
        .write_all(payload)
        .await
        .map_err(|error| WireError::internal(format!("writing frame payload: {error}")))?;
    writer
        .flush()
        .await
        .map_err(|error| WireError::internal(format!("flushing frame: {error}")))
}
