use std::time::Duration;

use tokio::io::duplex;

use super::*;

#[tokio::test]
async fn a_frame_round_trips() {
    let (mut writer, mut reader) = duplex(64);
    let payload = br#"{"type":"host_info","data":null}"#;

    write_frame(&mut writer, payload).await.expect("write");
    let read = read_frame(&mut reader).await.expect("read").expect("frame");
    assert_eq!(read, payload);
}

#[tokio::test]
async fn a_clean_eof_is_not_an_error() {
    let (writer, mut reader) = duplex(16);
    drop(writer);
    assert!(read_frame(&mut reader).await.expect("eof").is_none());
}

#[tokio::test]
async fn an_oversized_write_is_refused_before_any_bytes_are_sent() {
    let (mut writer, mut reader) = duplex(64);
    let error = write_frame_limited(&mut writer, &[0u8; 8], 4)
        .await
        .expect_err("refused");
    assert_eq!(error.kind, ErrorKind::TooLarge);
    assert_eq!(error.code, "frame_too_large");
    drop(writer);
    assert!(read_frame(&mut reader).await.expect("eof").is_none());
}

#[tokio::test]
async fn an_oversized_read_is_refused() {
    let (mut writer, mut reader) = duplex(64);
    let len = 8u32.to_be_bytes();
    tokio::io::AsyncWriteExt::write_all(&mut writer, &len)
        .await
        .expect("header");
    tokio::io::AsyncWriteExt::write_all(&mut writer, &[0u8; 8])
        .await
        .expect("payload");
    let error = read_frame_limited(&mut reader, 4)
        .await
        .expect_err("refused");
    assert_eq!(error.kind, ErrorKind::TooLarge);
    assert_eq!(error.code, "frame_too_large");
}

#[test]
fn interval_accepts_units_and_fractions() {
    assert_eq!(parse_interval("10ms").unwrap(), Duration::from_millis(10));
    assert_eq!(parse_interval("500ms").unwrap(), Duration::from_millis(500));
    assert_eq!(parse_interval("2s").unwrap(), Duration::from_secs(2));
    assert_eq!(parse_interval("1.5s").unwrap(), Duration::from_millis(1500));
    assert_eq!(parse_interval("1m").unwrap(), Duration::from_secs(60));
    assert_eq!(parse_interval("1h").unwrap(), Duration::from_secs(3600));
}

#[test]
fn interval_refuses_a_missing_unit_and_bounds() {
    assert!(parse_interval("2").is_err());
    assert!(parse_interval("9ms").is_err());
    assert!(parse_interval("0s").is_err());
    assert!(parse_interval("-1s").is_err());
    assert!(parse_interval("25h").is_err());
    assert!(parse_interval("nonsense").is_err());
}

#[test]
fn hello_and_welcome_round_trip() {
    let hello = Request::Hello {
        protocol_version: PROTOCOL_VERSION,
        client: "cli/0.3.0".to_string(),
        features: Vec::new(),
    };
    let text = serde_json::to_string(&hello).expect("serialize");
    assert_eq!(
        text,
        r#"{"type":"hello","data":{"protocol_version":2,"client":"cli/0.3.0","features":[]}}"#
    );
    assert_eq!(
        serde_json::from_str::<Request>(&text).expect("parse"),
        hello
    );

    let welcome = Response::Welcome {
        protocol_version: PROTOCOL_VERSION,
        server: "anth-panel".to_string(),
        version: "0.3.0".to_string(),
        features: Vec::new(),
    };
    let text = serde_json::to_string(&welcome).expect("serialize");
    assert_eq!(
        text,
        r#"{"type":"welcome","data":{"protocol_version":2,"server":"anth-panel","version":"0.3.0","features":[]}}"#
    );
    assert_eq!(
        serde_json::from_str::<Response>(&text).expect("parse"),
        welcome
    );
}

#[test]
fn page_requests_validate_limits_and_cursor_length() {
    assert!(PageRequest::default().valid());
    assert_eq!(PageRequest::default().limit(), DEFAULT_PAGE_LIMIT);
    assert!(
        PageRequest {
            limit: Some(1),
            cursor: None
        }
        .valid()
    );
    assert!(
        !PageRequest {
            limit: Some(0),
            cursor: None
        }
        .valid()
    );
    assert!(
        !PageRequest {
            limit: Some(MAX_PAGE_LIMIT + 1),
            cursor: None
        }
        .valid()
    );
    assert!(
        !PageRequest {
            limit: None,
            cursor: Some("x".repeat(MAX_CURSOR_LEN + 1))
        }
        .valid()
    );
}

#[test]
fn a_page_can_report_an_end_or_a_next_cursor() {
    let end: Page<u8> = Page::new(vec![1, 2]);
    assert_eq!(end.next, None);
    assert_eq!(end.total, None);

    let next = Page::with_next(vec![1], "cursor");
    assert_eq!(next.next.as_deref(), Some("cursor"));

    let total = Page::with_total(vec![1], 42);
    assert_eq!(total.total, Some(42));
}
