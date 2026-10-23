//! Tests for every module in this crate.
//!
//! One file per level, as the repository layout rule requires: the modules above
//! hold behaviour, and this holds what asserts it.

use super::*;

mod error {
    use super::*;

    #[test]
    fn every_kind_has_a_distinct_status_and_a_tag() {
        let kinds = [
            ErrorKind::NotFound,
            ErrorKind::Forbidden,
            ErrorKind::Conflict,
            ErrorKind::Invalid,
            ErrorKind::Unsupported,
            ErrorKind::TooLarge,
            ErrorKind::Internal,
        ];
        let mut tags: Vec<&str> = kinds.iter().map(|k| k.as_str()).collect();
        tags.sort_unstable();
        let unique = tags.len();
        tags.dedup();
        assert_eq!(tags.len(), unique, "two kinds share a tag");

        for kind in kinds {
            assert!(
                (400..=599).contains(&kind.http_status()),
                "{kind:?} has no status"
            );
        }
    }

    #[test]
    fn the_status_mapping_matches_the_documented_contract() {
        assert_eq!(ErrorKind::NotFound.http_status(), 404);
        assert_eq!(ErrorKind::Forbidden.http_status(), 403);
        assert_eq!(ErrorKind::Conflict.http_status(), 409);
        assert_eq!(ErrorKind::Invalid.http_status(), 400);
        assert_eq!(ErrorKind::Unsupported.http_status(), 501);
        assert_eq!(ErrorKind::TooLarge.http_status(), 413);
        assert_eq!(ErrorKind::Internal.http_status(), 500);
    }

    #[test]
    fn a_caller_mistake_exits_two_and_a_host_failure_exits_one() {
        assert_eq!(ErrorKind::Invalid.exit_code(), 2);
        assert_eq!(ErrorKind::NotFound.exit_code(), 1);
        assert_eq!(ErrorKind::Forbidden.exit_code(), 1);
    }

    #[test]
    fn an_error_round_trips_through_json() {
        let error = WireError::forbidden("control is off");
        let json = serde_json::to_string(&error).unwrap();
        assert_eq!(json, r#"{"kind":"forbidden","message":"control is off"}"#);
        let back: WireError = serde_json::from_str(&json).unwrap();
        assert_eq!(back, error);
    }

    #[test]
    fn display_names_the_kind_so_a_log_line_is_greppable() {
        let error = WireError::not_found("/etc/nope");
        assert_eq!(error.to_string(), "not_found: /etc/nope");
    }
}

mod frame {
    use super::*;
    use tokio::io::AsyncWriteExt;

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

mod message {
    use super::*;

    fn sample() -> HostInfo {
        HostInfo {
            hostname: "anth".into(),
            os: OsInfo {
                name: "Alpine Linux".into(),
                version: "3.20".into(),
                kernel: "6.6.0".into(),
                arch: "x86_64".into(),
            },
            uptime_seconds: 3600,
            cpu_model: "Example CPU".into(),
            logical_cores: 4,
            load: LoadAverage {
                one: 0.5,
                five: 0.25,
                fifteen: 0.125,
            },
            memory: MemoryInfo {
                total_kib: 1024,
                available_kib: 512,
                swap_total_kib: 0,
                swap_free_kib: 0,
            },
        }
    }

    #[test]
    fn a_request_round_trips_through_json() {
        let request = Request::HostInfo;
        let json = serde_json::to_string(&request).unwrap();
        assert_eq!(json, r#"{"type":"host_info"}"#);
        assert_eq!(serde_json::from_str::<Request>(&json).unwrap(), request);
    }

    #[test]
    fn hello_carries_the_version() {
        let request = Request::Hello {
            protocol_version: PROTOCOL_VERSION,
        };
        let json = serde_json::to_string(&request).unwrap();
        assert_eq!(json, r#"{"type":"hello","data":{"protocol_version":1}}"#);
        assert_eq!(serde_json::from_str::<Request>(&json).unwrap(), request);
    }

    #[test]
    fn a_snapshot_round_trips_through_json() {
        let response = Response::HostInfo(sample());
        let json = serde_json::to_string(&response).unwrap();
        let back: Response = serde_json::from_str(&json).unwrap();
        assert_eq!(back, response);
    }

    #[test]
    fn an_error_response_is_an_error_and_not_a_snapshot() {
        let response = Response::Error(WireError::forbidden("control is off"));
        assert!(response.error().is_some());
        assert!(response.into_result().is_err());

        let ok = Response::HostInfo(sample());
        assert!(ok.error().is_none());
        assert!(ok.into_result().is_ok());
    }

    #[test]
    fn the_wire_tag_matches_the_variant_name_in_snake_case() {
        // An alternative client depends on this spelling, so pin it.
        let response = Response::Welcome {
            protocol_version: PROTOCOL_VERSION,
        };
        assert_eq!(
            serde_json::to_string(&response).unwrap(),
            r#"{"type":"welcome","data":{"protocol_version":1}}"#
        );
    }

    #[test]
    fn used_memory_and_swap_never_go_negative() {
        let mut memory = sample().memory;
        assert_eq!(memory.used_kib(), 512);

        memory.available_kib = 4096;
        memory.swap_total_kib = 10;
        memory.swap_free_kib = 20;
        assert_eq!(memory.used_kib(), 0);
        assert_eq!(memory.swap_used_kib(), 0);
    }

    #[test]
    fn an_unknown_message_type_is_a_parse_error_not_a_default() {
        let result = serde_json::from_str::<Request>(r#"{"type":"nope"}"#);
        assert!(result.is_err(), "an unknown variant must not deserialize");
    }
}
