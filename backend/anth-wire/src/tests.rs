use std::time::Duration;

use super::*;

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
fn the_hello_body_names_the_protocol() {
    let hello = Hello {
        protocol_version: PROTOCOL_VERSION,
    };
    let text = serde_json::to_string(&hello).expect("serialize");
    assert_eq!(
        text,
        format!(r#"{{"protocol_version":{PROTOCOL_VERSION}}}"#)
    );
    assert_eq!(serde_json::from_str::<Hello>(&text).expect("parse"), hello);
}

#[test]
fn terminal_websocket_messages_round_trip() {
    let input = TerminalClientMessage::Input {
        data: "aGk=".to_string(),
    };
    let text = serde_json::to_string(&input).expect("serialize");
    assert_eq!(text, r#"{"type":"input","data":"aGk="}"#);
    assert_eq!(
        serde_json::from_str::<TerminalClientMessage>(&text).expect("parse"),
        input
    );

    let ready = TerminalServerMessage::Ready {
        shell: "/bin/sh".to_string(),
        cols: 80,
        rows: 24,
        line_editing: false,
    };
    let text = serde_json::to_string(&ready).expect("serialize");
    assert_eq!(
        text,
        r#"{"type":"ready","shell":"/bin/sh","cols":80,"rows":24,"line_editing":false}"#
    );
    assert_eq!(
        serde_json::from_str::<TerminalServerMessage>(&text).expect("parse"),
        ready
    );

    let exit = TerminalServerMessage::Exit { code: 0 };
    let text = serde_json::to_string(&exit).expect("serialize");
    assert_eq!(text, r#"{"type":"exit","code":0}"#);
    assert_eq!(
        serde_json::from_str::<TerminalServerMessage>(&text).expect("parse"),
        exit
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
