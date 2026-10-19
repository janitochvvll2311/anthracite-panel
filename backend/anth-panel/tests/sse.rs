use std::convert::Infallible;
use std::time::Duration;

use anth_panel::sse::SseStream;
use axum::body::Body;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use futures::stream;
use http_body_util::BodyExt;

/// Reads until `frames` events carrying a `data:` payload have arrived.
async fn read_frames(body: Body, frames: usize) -> String {
    let mut body = body;
    let mut text = String::new();

    while text.matches("\ndata: ").count() + usize::from(text.starts_with("data: ")) < frames {
        let next = tokio::time::timeout(Duration::from_secs(5), body.frame())
            .await
            .expect("frame should arrive");
        let Some(frame) = next else { break };
        let frame = frame.expect("body should not error");
        if let Some(data) = frame.data_ref() {
            text.push_str(&String::from_utf8_lossy(data));
        }
    }
    text
}

#[tokio::test]
async fn items_become_named_events_with_ids_and_a_retry_hint() {
    let items = stream::iter(vec![
        Ok::<_, Infallible>(serde_json::json!({ "n": 1 })),
        Ok(serde_json::json!({ "n": 2 })),
    ]);

    let response = SseStream::new("host-info").stream(items).into_response();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "text/event-stream");

    let text = read_frames(response.into_body(), 2).await;

    assert!(text.contains("retry: 3000"), "{text}");
    assert!(text.contains("event: host-info"), "{text}");
    assert!(text.contains(r#"data: {"n":1}"#), "{text}");
    assert!(text.contains("id: 1"), "{text}");
    assert!(text.contains("id: 2"), "{text}");
    // The retry hint is sent once, with the first event only.
    assert_eq!(text.matches("retry: 3000").count(), 1, "{text}");
}

#[tokio::test]
async fn an_error_becomes_an_error_event() {
    let items = stream::iter(vec![Err::<serde_json::Value, _>("daemon unavailable")]);

    let response = SseStream::new("host-info").stream(items).into_response();
    let text = read_frames(response.into_body(), 1).await;

    assert!(text.contains("event: error"), "{text}");
    assert!(text.contains("data: daemon unavailable"), "{text}");
}

#[tokio::test]
async fn the_stream_ends_after_the_source_ends() {
    let items = stream::iter(Vec::<Result<serde_json::Value, Infallible>>::new());

    let response = SseStream::new("host-info").stream(items).into_response();
    let body = response.into_body();
    let collected = tokio::time::timeout(Duration::from_secs(5), body.collect())
        .await
        .expect("an empty source must still terminate")
        .unwrap();

    assert!(collected.to_bytes().is_empty());
}

#[test]
fn keep_alive_is_fifteen_seconds() {
    assert_eq!(anth_panel::sse::KEEP_ALIVE, Duration::from_secs(15));
}
