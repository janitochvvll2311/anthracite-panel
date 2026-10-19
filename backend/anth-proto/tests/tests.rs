use std::time::{SystemTime, UNIX_EPOCH};

use anth_proto::{AnthClient, AnthReader, AnthServer, AnthWriter, HostInfo, Request, Response};
use tokio::net::{UnixListener, UnixStream};

fn unique_socket(name: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "anth-proto-{name}-{}-{nanos}.sock",
        std::process::id()
    ))
}

#[test]
fn echo_request_roundtrip_preserves_message() {
    let request = Request::Echo {
        message: "hola".to_string(),
    };

    let json = request.to_json().expect("serialization should work");
    let decoded = Request::from_json(&json).expect("deserialization should work");

    match decoded {
        Request::Echo { message } => assert_eq!(message, "hola"),
        other => panic!("expected Echo, got {other:?}"),
    }
}

#[test]
fn request_wire_format_is_tagged_with_data_payload() {
    let json = Request::Echo {
        message: "sync".to_string(),
    }
    .to_json()
    .unwrap();

    assert_eq!(json, r#"{"type":"Echo","data":{"message":"sync"}}"#);
}

#[test]
fn host_info_request_wire_format_has_no_payload() {
    assert_eq!(
        Request::HostInfo.to_json().unwrap(),
        r#"{"type":"HostInfo"}"#
    );
}

#[test]
fn response_wire_format_is_tagged_with_data_payload() {
    assert_eq!(
        Response::Ok {
            message: "ok".to_string()
        }
        .to_json()
        .unwrap(),
        r#"{"type":"Ok","data":{"message":"ok"}}"#
    );

    assert_eq!(
        Response::Error {
            detail: "boom".to_string()
        }
        .to_json()
        .unwrap(),
        r#"{"type":"Error","data":{"detail":"boom"}}"#
    );
}

#[test]
fn response_roundtrip_for_every_variant() {
    let cases = [
        Response::Ok {
            message: "ok".to_string(),
        },
        Response::Error {
            detail: "boom".to_string(),
        },
    ];

    for response in cases {
        let json = response.to_json().unwrap();
        let decoded = Response::from_json(&json).unwrap();
        assert_eq!(json, decoded.to_json().unwrap());
    }
}

#[test]
fn malformed_json_is_rejected() {
    assert!(Request::from_json("not json at all").is_err());
    assert!(Request::from_json("").is_err());
    assert!(Request::from_json(r#"{"type":"Nope"}"#).is_err());
    assert!(Request::from_json(r#"{"type":"Echo","data":{}}"#).is_err());
    assert!(Response::from_json("not json at all").is_err());
}

#[test]
fn host_info_roundtrips_over_the_wire() {
    let info = HostInfo {
        hostname: "box".to_string(),
        os: anth_proto::OsInfo {
            name: "Debian GNU/Linux".to_string(),
            version: "13".to_string(),
            kernel: "6.12.0".to_string(),
            arch: "x86_64".to_string(),
        },
        uptime_seconds: 90_061,
        cpu_model: "Intel".to_string(),
        logical_cores: 8,
        load: anth_proto::LoadAverage {
            one: 0.5,
            five: 0.25,
            fifteen: 0.1,
        },
        memory: anth_proto::MemoryInfo {
            total_kib: 8_000_000,
            available_kib: 4_000_000,
            swap_total_kib: 1_000,
            swap_free_kib: 400,
        },
    };

    let response = Response::HostInfo(Box::new(info.clone()));
    let json = response.to_json().unwrap();
    assert!(json.starts_with(r#"{"type":"HostInfo","data":{"hostname":"box""#));

    match Response::from_json(&json).unwrap() {
        Response::HostInfo(decoded) => assert_eq!(*decoded, info),
        other => panic!("expected HostInfo, got {other:?}"),
    }
}

#[test]
fn unicode_and_escapes_survive_roundtrip() {
    let text = "café \"quoted\"\\ line\nbreak 🚀".to_string();
    let request = Request::Echo {
        message: text.clone(),
    };

    let decoded = Request::from_json(&request.to_json().unwrap()).unwrap();
    match decoded {
        Request::Echo { message } => assert_eq!(message, text),
        other => panic!("expected Echo, got {other:?}"),
    }
}

#[tokio::test]
async fn server_and_client_exchange_an_echo_over_a_socket() {
    let path = unique_socket("exchange");
    let _ = std::fs::remove_file(&path);

    let listener = UnixListener::bind(&path).unwrap();
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let mut server = AnthServer::new(socket);

        if let Some(Ok(request)) = server.receive_request().await {
            let response = match request {
                Request::Echo { message } => Response::Ok { message },
                other => Response::Error {
                    detail: format!("unexpected request: {other:?}"),
                },
            };
            server.send_response(&response).await.unwrap();
        }
    });

    let stream = UnixStream::connect(&path).await.unwrap();
    let mut client = AnthClient::new(stream);
    client
        .send_request(&Request::Echo {
            message: "ping".to_string(),
        })
        .await
        .unwrap();

    let response = client.receive_response().await.unwrap().unwrap();
    match response {
        Response::Ok { message } => assert_eq!(message, "ping"),
        other => panic!("expected Ok, got {other:?}"),
    }

    server.await.unwrap();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn server_and_client_exchange_a_host_info_request() {
    let path = unique_socket("host-info");
    let _ = std::fs::remove_file(&path);

    let listener = UnixListener::bind(&path).unwrap();
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let mut server = AnthServer::new(socket);

        if let Some(Ok(request)) = server.receive_request().await {
            assert!(matches!(request, Request::HostInfo));
            let response = Response::HostInfo(Box::default());
            server.send_response(&response).await.unwrap();
        }
    });

    let stream = UnixStream::connect(&path).await.unwrap();
    let mut client = AnthClient::new(stream);
    client.send_request(&Request::HostInfo).await.unwrap();

    match client.receive_response().await.unwrap().unwrap() {
        Response::HostInfo(info) => assert_eq!(info.hostname, ""),
        other => panic!("expected HostInfo, got {other:?}"),
    }

    server.await.unwrap();
    let _ = std::fs::remove_file(&path);
}

#[test]
fn terminal_bytes_survive_the_line_framing() {
    // These are the inputs that broke `LinesCodec` when terminal bytes travelled
    // as text: a lone CR, and bytes that are not valid UTF-8.
    let bytes = vec![0x1b, b'[', b'H', b'\r', 0xff, 0x80, 0x00, b'\n', 0x07];

    let request = Request::TerminalInput {
        id: 7,
        bytes: bytes.clone(),
    };
    let json = request.to_json().unwrap();

    assert!(
        !json.contains('\n'),
        "a terminal frame must not contain a raw newline: {json}"
    );

    match Request::from_json(&json).unwrap() {
        Request::TerminalInput { id, bytes: decoded } => {
            assert_eq!(id, 7);
            assert_eq!(decoded, bytes, "bytes must round-trip unchanged");
        }
        other => panic!("expected TerminalInput, got {other:?}"),
    }
}

#[test]
fn terminal_output_wire_format_is_base64() {
    let response = Response::TerminalOutput {
        id: 1,
        bytes: vec![b'h', b'i'],
    };

    assert_eq!(
        response.to_json().unwrap(),
        r#"{"type":"TerminalOutput","data":{"id":1,"bytes":"aGk="}}"#
    );
}

#[test]
fn terminal_open_and_opened_roundtrip() {
    let request = Request::TerminalOpen(anth_proto::TerminalOpenArgs {
        size: Some(anth_proto::TerminalSize {
            cols: 120,
            rows: 40,
        }),
    });

    let json = request.to_json().unwrap();
    assert!(json.contains(r#""size":{"cols":120,"rows":40}"#), "{json}");
    match Request::from_json(&json).unwrap() {
        Request::TerminalOpen(args) => {
            assert_eq!(args.size.unwrap().cols, 120);
            assert_eq!(args.size.unwrap().rows, 40);
        }
        other => panic!("expected TerminalOpen, got {other:?}"),
    }

    let opened = Response::TerminalOpened(anth_proto::TerminalOpened {
        id: 3,
        size: anth_proto::TerminalSize { cols: 80, rows: 24 },
        program: "/bin/sh".to_string(),
        line_editing: false,
    });
    match Response::from_json(&opened.to_json().unwrap()).unwrap() {
        Response::TerminalOpened(o) => {
            assert_eq!(o.id, 3);
            assert_eq!(o.program, "/bin/sh");
            assert_eq!(o.size.rows, 24);
            assert!(!o.line_editing);
        }
        other => panic!("expected TerminalOpened, got {other:?}"),
    }
}

#[test]
fn terminal_open_without_a_size_omits_the_member() {
    let json = Request::TerminalOpen(anth_proto::TerminalOpenArgs::default())
        .to_json()
        .unwrap();
    assert_eq!(json, r#"{"type":"TerminalOpen","data":{}}"#);
    assert!(matches!(
        Request::from_json(&json).unwrap(),
        Request::TerminalOpen(_)
    ));
}

#[test]
fn terminal_sizes_outside_the_sane_range_are_rejected_by_the_check() {
    use anth_proto::TerminalSize;

    assert!(TerminalSize { cols: 80, rows: 24 }.is_sane());
    assert!(!TerminalSize { cols: 0, rows: 24 }.is_sane());
    assert!(!TerminalSize { cols: 80, rows: 0 }.is_sane());
    assert!(!TerminalSize {
        cols: 1000,
        rows: 1001
    }
    .is_sane());
    assert!(TerminalSize::MAX.is_sane());
}

#[tokio::test]
async fn a_line_longer_than_the_cap_is_an_error_not_an_allocation() {
    use tokio_util::codec::Decoder;

    let mut codec = anth_proto::codec();
    // One byte past the cap, with no newline: the old `LinesCodec::new()` would
    // have kept buffering this forever.
    let mut buffer = bytes::BytesMut::from(&vec![b'x'; anth_proto::MAX_LINE_LEN + 1][..]);

    let result = codec.decode(&mut buffer);
    assert!(
        result.is_err(),
        "a line over the cap must be rejected, got {result:?}"
    );
}

#[tokio::test]
async fn split_reader_and_writer_carry_the_client_direction() {
    let path = unique_socket("split-client");
    let _ = std::fs::remove_file(&path);

    let listener = UnixListener::bind(&path).unwrap();
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let mut server = AnthServer::new(socket);

        if let Some(Ok(request)) = server.receive_request().await {
            let response = match request {
                Request::Echo { message } => Response::Ok { message },
                other => Response::Error {
                    detail: format!("unexpected request: {other:?}"),
                },
            };
            server.send_response(&response).await.unwrap();
        }
    });

    let stream = UnixStream::connect(&path).await.unwrap();
    let (read_half, write_half) = stream.into_split();
    let mut reader = AnthReader::from_read_half(read_half);
    let mut writer = AnthWriter::from_write_half(write_half);

    writer
        .send_request(&Request::Echo {
            message: "split".to_string(),
        })
        .await
        .unwrap();

    match reader.receive_response().await.unwrap().unwrap() {
        Response::Ok { message } => assert_eq!(message, "split"),
        other => panic!("expected Ok, got {other:?}"),
    }

    server.await.unwrap();
    let _ = std::fs::remove_file(&path);
}

#[test]
fn base64_helpers_roundtrip_arbitrary_bytes() {
    let bytes = [0x00, 0x0a, 0x0d, 0xff, 0xfe];
    let encoded = anth_proto::b64::encode(&bytes);
    assert_eq!(anth_proto::b64::decode(&encoded).unwrap(), bytes);
    assert!(anth_proto::b64::decode("not base64!").is_err());
}

#[test]
fn process_requests_use_the_shared_tagged_envelope() {
    assert_eq!(
        Request::ProcessList.to_json().unwrap(),
        r#"{"type":"ProcessList"}"#
    );

    let signal = Request::ProcessSignal {
        pid: 42,
        signal: anth_proto::ProcessSignal::Term,
    };
    assert_eq!(
        signal.to_json().unwrap(),
        r#"{"type":"ProcessSignal","data":{"pid":42,"signal":"term"}}"#
    );
}

#[test]
fn process_snapshot_and_signal_responses_roundtrip() {
    let snapshot = anth_proto::ProcessSnapshot {
        processes: vec![anth_proto::ProcessInfo {
            pid: 42,
            ppid: 1,
            uid: 1000,
            user: "alice".to_string(),
            name: "sleep".to_string(),
            command: "sleep 30".to_string(),
            state: "S".to_string(),
            cpu_ticks: 12,
            rss_kib: 2048,
            threads: 1,
            start_ticks: 12345,
        }],
        clock_ticks: 100,
        timestamp_ms: 1_700_000_000_000,
        control_enabled: false,
        total: 1,
        truncated: false,
        warning: None,
    };
    let response = Response::ProcessSnapshot(snapshot.clone());
    match Response::from_json(&response.to_json().unwrap()).unwrap() {
        Response::ProcessSnapshot(decoded) => assert_eq!(decoded, snapshot),
        other => panic!("expected ProcessSnapshot, got {other:?}"),
    }

    let signalled = Response::ProcessSignalled {
        pid: 42,
        signal: anth_proto::ProcessSignal::Kill,
    };
    match Response::from_json(&signalled.to_json().unwrap()).unwrap() {
        Response::ProcessSignalled { pid, signal } => {
            assert_eq!(pid, 42);
            assert_eq!(signal, anth_proto::ProcessSignal::Kill);
        }
        other => panic!("expected ProcessSignalled, got {other:?}"),
    }
}

#[test]
fn process_signal_names_are_case_insensitive() {
    use anth_proto::ProcessSignal;
    use std::str::FromStr;

    assert_eq!(
        ProcessSignal::from_str("TERM").unwrap(),
        ProcessSignal::Term
    );
    assert_eq!(
        ProcessSignal::from_str("Kill").unwrap(),
        ProcessSignal::Kill
    );
    assert_eq!(
        ProcessSignal::from_str("cont").unwrap(),
        ProcessSignal::Cont
    );
    assert!(ProcessSignal::from_str("nope").is_err());
}
