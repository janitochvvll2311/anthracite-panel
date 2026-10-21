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
        spawn_enabled: true,
        spawn_allow: vec!["/bin/echo".to_string()],
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
fn an_empty_spawn_allowlist_is_still_serialised() {
    // An empty list means "spawn is closed", which the panel must be able to
    // read. Dropping the key made the browser see `undefined` and throw while
    // rendering the process panel, so the deny-all state has to be on the wire.
    let snapshot = anth_proto::ProcessSnapshot {
        spawn_enabled: true,
        spawn_allow: Vec::new(),
        ..Default::default()
    };

    let json = Response::ProcessSnapshot(snapshot).to_json().unwrap();

    assert!(
        json.contains(r#""spawn_allow":[]"#),
        "an empty allowlist must be serialised, got {json}"
    );
}

#[test]
fn network_requests_use_the_shared_tagged_envelope() {
    assert_eq!(
        Request::NetworkList.to_json().unwrap(),
        r#"{"type":"NetworkList"}"#
    );

    let change = Request::NetworkSetState {
        name: "eth0".to_string(),
        up: false,
    };
    assert_eq!(
        change.to_json().unwrap(),
        r#"{"type":"NetworkSetState","data":{"name":"eth0","up":false}}"#
    );
}

#[test]
fn network_snapshot_and_state_responses_roundtrip() {
    let snapshot = anth_proto::NetworkSnapshot {
        interfaces: vec![anth_proto::NetworkInterface {
            name: "eth0".to_string(),
            index: 2,
            state: "up".to_string(),
            up: true,
            running: true,
            loopback: false,
            mac: "aa:bb:cc:dd:ee:ff".to_string(),
            mtu: 1500,
            speed_mbps: Some(1000),
            duplex: Some("full".to_string()),
            addresses: vec![anth_proto::NetworkAddress {
                family: anth_proto::NetworkAddressFamily::Ipv4,
                address: "192.0.2.10".to_string(),
                prefix_len: 24,
            }],
            rx_bytes: 10,
            tx_bytes: 20,
            rx_packets: 1,
            tx_packets: 2,
            rx_errors: 0,
            tx_errors: 0,
            rx_dropped: 0,
            tx_dropped: 0,
        }],
        timestamp_ms: 1_700_000_000_000,
        control_enabled: true,
        total: 1,
        truncated: false,
        warning: None,
    };
    let response = Response::NetworkSnapshot(snapshot.clone());
    match Response::from_json(&response.to_json().unwrap()).unwrap() {
        Response::NetworkSnapshot(decoded) => assert_eq!(decoded, snapshot),
        other => panic!("expected NetworkSnapshot, got {other:?}"),
    }

    let changed = Response::NetworkStateChanged {
        name: "eth0".to_string(),
        up: true,
    };
    match Response::from_json(&changed.to_json().unwrap()).unwrap() {
        Response::NetworkStateChanged { name, up } => {
            assert_eq!(name, "eth0");
            assert!(up);
        }
        other => panic!("expected NetworkStateChanged, got {other:?}"),
    }

    let error = Response::NetworkError(anth_proto::NetworkError::new(
        anth_proto::NetworkErrorKind::Protected,
        "refusing to bring the loopback interface down",
    ));
    match Response::from_json(&error.to_json().unwrap()).unwrap() {
        Response::NetworkError(decoded) => {
            assert_eq!(decoded.kind, anth_proto::NetworkErrorKind::Protected);
        }
        other => panic!("expected NetworkError, got {other:?}"),
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

#[test]
fn file_requests_roundtrip() {
    let requests = [
        Request::FileList {
            path: "/tmp".to_string(),
        },
        Request::FileRead {
            path: "/tmp/file".to_string(),
            offset: 10,
            len: 20,
        },
        Request::FileMkdir {
            path: "/tmp/new".to_string(),
            recursive: true,
        },
        Request::FileRename {
            from: "/tmp/a".to_string(),
            to: "/tmp/b".to_string(),
        },
        Request::FileDelete {
            path: "/tmp/a".to_string(),
            recursive: false,
        },
        Request::FileWriteStart {
            path: "/tmp/upload".to_string(),
            overwrite: true,
            require_existing: false,
            expected_modified_ms: None,
        },
        Request::FileWriteStart {
            path: "/tmp/edit.txt".to_string(),
            overwrite: true,
            require_existing: true,
            expected_modified_ms: Some(1_700_000_000_000),
        },
        Request::FileWriteChunk {
            id: 3,
            bytes: vec![0, 1, 2, 255],
        },
        Request::FileWriteFinish { id: 3 },
        Request::FileWriteAbort { id: 3 },
    ];

    for request in requests {
        let json = request.to_json().unwrap();
        let decoded = Request::from_json(&json).unwrap();
        assert_eq!(json, decoded.to_json().unwrap());
    }
}

#[test]
fn file_response_roundtrip_and_wire_format() {
    let listing = anth_proto::FileListing {
        path: "/".to_string(),
        parent: None,
        entries: vec![anth_proto::FileEntry {
            name: "a.txt".to_string(),
            path: "/a.txt".to_string(),
            kind: anth_proto::FileKind::File,
            size: 3,
            modified_ms: Some(1_700_000_000_000),
            hidden: false,
            target: None,
        }],
        total: 1,
        truncated: false,
        warning: None,
        write_enabled: true,
        trash_enabled: false,
    };

    let responses = [
        Response::FileListing(listing),
        Response::FileChunk(anth_proto::FileChunk {
            path: "/a.txt".to_string(),
            offset: 0,
            size: 3,
            eof: true,
            bytes: b"abc".to_vec(),
        }),
        Response::FileWritten(anth_proto::FileWritten {
            path: "/a.txt".to_string(),
            size: 3,
            modified_ms: 1_700_000_000_000,
        }),
        Response::FileCreated {
            path: "/new".to_string(),
        },
        Response::FileDeleted {
            path: "/a.txt".to_string(),
        },
        Response::FileRenamed {
            from: "/a".to_string(),
            to: "/b".to_string(),
        },
        Response::FileWriteStarted { id: 1 },
        Response::FileWriteAccepted { id: 1, received: 3 },
        Response::FileWriteAborted { id: 1 },
        Response::FileError(anth_proto::FileError::new(
            anth_proto::FileErrorKind::NotFound,
            "missing",
        )),
    ];

    for response in responses {
        let json = response.to_json().unwrap();
        let decoded = Response::from_json(&json).unwrap();
        assert_eq!(json, decoded.to_json().unwrap());
    }

    let chunk = Response::FileChunk(anth_proto::FileChunk {
        path: "/a".to_string(),
        offset: 0,
        size: 3,
        eof: true,
        bytes: b"abc".to_vec(),
    });
    assert!(chunk.to_json().unwrap().contains(r#""bytes":"YWJj""#));
}

#[test]
fn firewall_requests_use_the_shared_tagged_envelope() {
    assert_eq!(
        Request::FirewallStatus.to_json().unwrap(),
        r#"{"type":"FirewallStatus"}"#
    );

    let add = Request::FirewallAddRule {
        action: anth_proto::FirewallAction::Allow,
        port: 8080,
        protocol: anth_proto::FirewallProtocol::Tcp,
        comment: Some("web".to_string()),
    };
    assert_eq!(
        add.to_json().unwrap(),
        r#"{"type":"FirewallAddRule","data":{"action":"allow","port":8080,"protocol":"tcp","comment":"web"}}"#
    );

    let delete = Request::FirewallDeleteRule { handle: 3 };
    assert_eq!(
        delete.to_json().unwrap(),
        r#"{"type":"FirewallDeleteRule","data":{"handle":3}}"#
    );
}

#[test]
fn firewall_snapshot_and_error_responses_roundtrip() {
    let snapshot = anth_proto::FirewallSnapshot {
        initialized: true,
        control_enabled: true,
        rules: vec![anth_proto::FirewallRule {
            handle: 3,
            chain: "input".to_string(),
            action: "allow".to_string(),
            protocol: Some(anth_proto::FirewallProtocol::Tcp),
            port: Some(22),
            comment: Some("ssh".to_string()),
        }],
        ruleset: "table inet anthracite".to_string(),
        total: 1,
        truncated: false,
        warning: None,
    };
    let response = Response::FirewallSnapshot(snapshot.clone());
    match Response::from_json(&response.to_json().unwrap()).unwrap() {
        Response::FirewallSnapshot(decoded) => assert_eq!(decoded, snapshot),
        other => panic!("expected FirewallSnapshot, got {other:?}"),
    }

    let truncated = anth_proto::FirewallSnapshot {
        truncated: true,
        warning: Some("showing 1 of 6000 firewall rules".to_string()),
        ..snapshot
    };
    let json = Response::FirewallSnapshot(truncated).to_json().unwrap();
    assert!(json.contains(r#""truncated":true"#), "{json}");
    assert!(json.contains(r#""warning":"showing 1 of 6000"#), "{json}");

    let changed = Response::FirewallChanged {
        message: "allowed tcp/22".to_string(),
    };
    match Response::from_json(&changed.to_json().unwrap()).unwrap() {
        Response::FirewallChanged { message } => assert_eq!(message, "allowed tcp/22"),
        other => panic!("expected FirewallChanged, got {other:?}"),
    }

    let error = Response::FirewallError(anth_proto::FirewallError::new(
        anth_proto::FirewallErrorKind::Unavailable,
        "nft is not installed",
    ));
    match Response::from_json(&error.to_json().unwrap()).unwrap() {
        Response::FirewallError(decoded) => {
            assert_eq!(decoded.kind, anth_proto::FirewallErrorKind::Unavailable);
        }
        other => panic!("expected FirewallError, got {other:?}"),
    }
}

#[test]
fn firewall_names_are_case_insensitive() {
    use std::str::FromStr;

    assert_eq!(
        anth_proto::FirewallAction::from_str("ALLOW").unwrap(),
        anth_proto::FirewallAction::Allow
    );
    assert_eq!(
        anth_proto::FirewallProtocol::from_str("UDP").unwrap(),
        anth_proto::FirewallProtocol::Udp
    );
    assert!(anth_proto::FirewallAction::from_str("nope").is_err());
    assert!(anth_proto::FirewallProtocol::from_str("sctp").is_err());
}

#[test]
fn volume_requests_use_the_shared_tagged_envelope() {
    assert_eq!(
        Request::VolumeList.to_json().unwrap(),
        r#"{"type":"VolumeList"}"#
    );

    let mount = Request::VolumeMount {
        device: "/dev/sdb1".to_string(),
        target: "/mnt/data".to_string(),
        fs_type: Some("ext4".to_string()),
        read_only: true,
        allow_removable: false,
    };
    assert_eq!(
        mount.to_json().unwrap(),
        r#"{"type":"VolumeMount","data":{"device":"/dev/sdb1","target":"/mnt/data","fs_type":"ext4","read_only":true,"allow_removable":false}}"#
    );

    let minimal = Request::VolumeMount {
        device: "/dev/sdb1".to_string(),
        target: "/mnt/data".to_string(),
        fs_type: None,
        read_only: false,
        allow_removable: false,
    };
    assert_eq!(
        minimal.to_json().unwrap(),
        r#"{"type":"VolumeMount","data":{"device":"/dev/sdb1","target":"/mnt/data","read_only":false,"allow_removable":false}}"#
    );

    let unmount = Request::VolumeUnmount {
        target: "/mnt/data".to_string(),
    };
    assert_eq!(
        unmount.to_json().unwrap(),
        r#"{"type":"VolumeUnmount","data":{"target":"/mnt/data"}}"#
    );
}

#[test]
fn volume_snapshot_and_error_responses_roundtrip() {
    let snapshot = anth_proto::VolumeSnapshot {
        devices: vec![anth_proto::VolumeDevice {
            name: "sda1".to_string(),
            path: "/dev/sda1".to_string(),
            kind: anth_proto::VolumeKind::Part,
            parent: Some("sda".to_string()),
            size_bytes: 512_000_000,
            fstype: Some("ext4".to_string()),
            label: Some("root".to_string()),
            uuid: Some("1111-2222".to_string()),
            mountpoints: vec!["/".to_string()],
            read_only: false,
            removable: false,
            model: None,
            transport: Some("sata".to_string()),
        }],
        mounts: vec![anth_proto::VolumeMount {
            mountpoint: "/".to_string(),
            source: "/dev/sda1".to_string(),
            fstype: "ext4".to_string(),
            options: "rw,relatime".to_string(),
            read_only: false,
            virtual_fs: false,
            usage: Some(anth_proto::VolumeUsage {
                total_bytes: 1000,
                used_bytes: 400,
                available_bytes: 550,
            }),
        }],
        control_enabled: true,
        timestamp_ms: 1_700_000_000_000,
        total: 1,
        truncated: false,
        warning: None,
    };

    let response = Response::VolumeSnapshot(snapshot.clone());
    match Response::from_json(&response.to_json().unwrap()).unwrap() {
        Response::VolumeSnapshot(decoded) => assert_eq!(decoded, snapshot),
        other => panic!("expected VolumeSnapshot, got {other:?}"),
    }

    let changed = Response::VolumeChanged {
        message: "mounted /dev/sdb1 on /mnt/data".to_string(),
    };
    match Response::from_json(&changed.to_json().unwrap()).unwrap() {
        Response::VolumeChanged { message } => {
            assert_eq!(message, "mounted /dev/sdb1 on /mnt/data")
        }
        other => panic!("expected VolumeChanged, got {other:?}"),
    }

    let error = Response::VolumeError(anth_proto::VolumeError::new(
        anth_proto::VolumeErrorKind::Busy,
        "target is busy",
    ));
    let json = error.to_json().unwrap();
    assert!(json.contains(r#""kind":"busy""#), "{json}");
    match Response::from_json(&json).unwrap() {
        Response::VolumeError(error) => {
            assert_eq!(error.kind, anth_proto::VolumeErrorKind::Busy)
        }
        other => panic!("expected VolumeError, got {other:?}"),
    }
}

#[test]
fn volume_devices_always_serialize_mountpoints() {
    let device = anth_proto::VolumeDevice {
        name: "sdb1".to_string(),
        path: "/dev/sdb1".to_string(),
        kind: anth_proto::VolumeKind::Part,
        ..Default::default()
    };
    assert!(device.mountpoints.is_empty());

    let json = serde_json::to_string(&device).unwrap();
    assert!(
        json.contains(r#""mountpoints":[]"#),
        "an unmounted device must still carry the key: {json}"
    );

    let decoded: anth_proto::VolumeDevice = serde_json::from_str(&json).unwrap();
    assert!(decoded.mountpoints.is_empty());
}

#[test]
fn volume_kinds_match_lsblk_names() {
    use std::str::FromStr;

    for (name, kind) in [
        ("disk", anth_proto::VolumeKind::Disk),
        ("part", anth_proto::VolumeKind::Part),
        ("crypt", anth_proto::VolumeKind::Crypt),
        ("lvm", anth_proto::VolumeKind::Lvm),
        ("raid1", anth_proto::VolumeKind::Raid),
        ("loop", anth_proto::VolumeKind::Loop),
        ("rom", anth_proto::VolumeKind::Rom),
        ("whatever", anth_proto::VolumeKind::Other),
    ] {
        assert_eq!(
            anth_proto::VolumeKind::from_str(name).unwrap(),
            kind,
            "{name}"
        );
    }
    assert_eq!(
        serde_json::to_string(&anth_proto::VolumeKind::Part).unwrap(),
        r#""part""#
    );
}
