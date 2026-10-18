use std::time::{SystemTime, UNIX_EPOCH};

use anth_proto::{AnthClient, AnthServer, HostInfo, Request, Response};
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
        Request::HostInfo => panic!("expected Echo, got HostInfo"),
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
        Request::HostInfo => panic!("expected Echo, got HostInfo"),
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
                Request::HostInfo => Response::Error {
                    detail: "unexpected host info request".to_string(),
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
