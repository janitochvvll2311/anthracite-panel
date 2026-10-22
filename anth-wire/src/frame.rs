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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_payload_round_trips_through_a_pipe() {
        let (mut a, mut b) = tokio::io::duplex(4096);
        let payload = br#"{"type":"host_info"}"#;
        write_frame(&mut a, payload).await.unwrap();
        let got = read_frame(&mut b).await.unwrap().unwrap();
        assert_eq!(got, payload);
    }

    #[tokio::test]
    async fn a_payload_with_newlines_survives() {
        // The v0.1.0 line framing could not carry this without base64; a
        // length-prefixed frame does not care.
        let (mut a, mut b) = tokio::io::duplex(4096);
        let payload = b"line one\nline two\n\nline four";
        write_frame(&mut a, payload).await.unwrap();
        assert_eq!(read_frame(&mut b).await.unwrap().unwrap(), payload);
    }

    #[tokio::test]
    async fn an_empty_payload_is_a_valid_frame() {
        let (mut a, mut b) = tokio::io::duplex(64);
        write_frame(&mut a, b"").await.unwrap();
        assert_eq!(read_frame(&mut b).await.unwrap().unwrap(), b"");
    }

    #[tokio::test]
    async fn several_frames_in_a_row_stay_separate() {
        let (mut a, mut b) = tokio::io::duplex(4096);
        for payload in [b"one".as_slice(), b"two", b"three"] {
            write_frame(&mut a, payload).await.unwrap();
        }
        for expected in [b"one".as_slice(), b"two", b"three"] {
            assert_eq!(read_frame(&mut b).await.unwrap().unwrap(), expected);
        }
    }

    #[tokio::test]
    async fn a_closed_stream_reads_as_end_of_stream() {
        let (a, mut b) = tokio::io::duplex(64);
        drop(a);
        assert!(read_frame(&mut b).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn a_truncated_payload_is_an_error_not_a_short_read() {
        let (mut a, mut b) = tokio::io::duplex(64);
        // Header promises 10 bytes, then the peer disappears after 3.
        a.write_all(&10u32.to_be_bytes()).await.unwrap();
        a.write_all(b"abc").await.unwrap();
        drop(a);
        let error = read_frame(&mut b).await.unwrap_err();
        assert_eq!(error.kind, ErrorKind::Internal);
    }

    #[tokio::test]
    async fn an_oversized_frame_is_refused_before_it_is_read() {
        let (mut a, mut b) = tokio::io::duplex(64);
        a.write_all(&100u32.to_be_bytes()).await.unwrap();
        let error = read_frame_limited(&mut b, 10).await.unwrap_err();
        assert_eq!(error.kind, ErrorKind::TooLarge);
    }

    #[tokio::test]
    async fn an_oversized_payload_is_refused_before_anything_is_written() {
        let (mut a, mut b) = tokio::io::duplex(64);
        let error = write_frame_limited(&mut a, b"0123456789", 5)
            .await
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::TooLarge);
        drop(a);
        assert!(
            read_frame(&mut b).await.unwrap().is_none(),
            "a rejected write must leave no header behind"
        );
    }

    #[tokio::test]
    async fn a_frame_larger_than_the_old_line_cap_round_trips() {
        // Regression for lesson 1: v0.1.0 capped a message at 1 MiB, and five
        // separate modules dropped data to stay under it. 20 MiB must pass
        // through untouched.
        let (mut a, mut b) = tokio::io::duplex(1024);
        let payload = vec![b'x'; 20 * 1024 * 1024];
        let expected = payload.clone();
        let writer = tokio::spawn(async move { write_frame(&mut a, &payload).await });
        let got = read_frame(&mut b).await.unwrap().unwrap();
        writer.await.unwrap().unwrap();
        assert_eq!(got.len(), 20 * 1024 * 1024);
        assert_eq!(got, expected);
    }
}
