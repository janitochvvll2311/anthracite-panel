//! The authentication layer seen through the real router.
//!
//! These tests use `tower::ServiceExt::oneshot` against `anth_panel::router`, so
//! they exercise the middleware order, the cookie headers and the JSON shape the
//! UI consumes, not just the service functions.

use std::sync::Arc;

use anth_panel::AppState;
use anth_panel::auth::password;
use anth_panel::config::{AuthConfig, Config};
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

fn config_with_auth(password: &str) -> Config {
    Config {
        auth: AuthConfig {
            enabled: true,
            username: "admin".to_string(),
            password_hash: password::hash_password_with_iterations(
                password,
                password::DEV_ITERATIONS,
            )
            .unwrap(),
            jwt_secret: "test-secret".to_string(),
            access_token_secs: 900,
            refresh_token_secs: 3600,
            iterations: password::DEV_ITERATIONS,
            cookie_secure: false,
            bootstrap_password: None,
        },
        ..Config::default()
    }
}

fn state(password: &str) -> AppState {
    AppState::new(config_with_auth(password))
}

fn json_request(method: &str, uri: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn request(uri: &str) -> Request<Body> {
    Request::builder().uri(uri).body(Body::empty()).unwrap()
}

fn cookie_header(response: &axum::http::Response<Body>, name: &str) -> Option<String> {
    response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .find(|value| value.starts_with(&format!("{name}=")))
        .map(|value| value.to_string())
}

fn cookie_value(set_cookie: &str) -> String {
    set_cookie
        .split(';')
        .next()
        .unwrap()
        .split_once('=')
        .unwrap()
        .1
        .to_string()
}

async fn body_json(response: axum::http::Response<Body>) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}

async fn login_access_cookie(app: &axum::Router) -> String {
    let login = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/auth/login",
            serde_json::json!({ "username": "admin", "password": "hunter2" }),
        ))
        .await
        .unwrap();
    cookie_value(&cookie_header(&login, "anth_access").unwrap())
}

#[tokio::test]
async fn an_unauthenticated_api_call_is_refused() {
    let app = anth_panel::router(state("hunter2"));
    let response = app.oneshot(request("/api/processes")).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let body = body_json(response).await;
    assert!(body["error"].as_str().unwrap().contains("authentication"));
}

#[tokio::test]
async fn the_health_probe_stays_public() {
    let app = anth_panel::router(state("hunter2"));
    let response = app.oneshot(request("/healthz")).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn a_disabled_layer_serves_every_route() {
    let app = anth_panel::router(AppState::new(Config::default()));
    // The route fails for another reason (no daemon), but not for auth.
    let response = app.oneshot(request("/api/processes")).await.unwrap();
    assert_ne!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn login_sets_both_cookies() {
    let app = anth_panel::router(state("hunter2"));
    let response = app
        .oneshot(json_request(
            "POST",
            "/api/auth/login",
            serde_json::json!({ "username": "admin", "password": "hunter2" }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let access = cookie_header(&response, "anth_access").expect("an access cookie");
    let refresh = cookie_header(&response, "anth_refresh").expect("a refresh cookie");
    assert!(access.contains("HttpOnly"));
    assert!(access.contains("SameSite=Strict"));
    assert!(!access.contains("Secure"));
    assert!(!refresh.is_empty());
}

#[tokio::test]
async fn a_wrong_password_is_refused_without_naming_the_field() {
    let app = anth_panel::router(state("hunter2"));
    let response = app
        .oneshot(json_request(
            "POST",
            "/api/auth/login",
            serde_json::json!({ "username": "admin", "password": "nope" }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(body_json(response).await["error"], "invalid credentials");
}

#[tokio::test]
async fn an_unknown_user_gets_the_same_answer_as_a_wrong_password() {
    let app = anth_panel::router(state("hunter2"));
    let response = app
        .oneshot(json_request(
            "POST",
            "/api/auth/login",
            serde_json::json!({ "username": "someone", "password": "hunter2" }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(body_json(response).await["error"], "invalid credentials");
}

#[tokio::test]
async fn a_valid_access_cookie_opens_a_protected_route() {
    let app = anth_panel::router(state("hunter2"));
    let access = login_access_cookie(&app).await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/processes")
                .header(header::COOKIE, format!("anth_access={access}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Without a daemon the route answers 503; the point is that it was reached.
    assert_ne!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_bearer_token_is_accepted_too() {
    let state = state("hunter2");
    let app = anth_panel::router(state.clone());
    let access = login_access_cookie(&app).await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/processes")
                .header(header::AUTHORIZATION, format!("Bearer {access}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_ne!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(state.auth().await.session_count(), 1);
}

#[tokio::test]
async fn refresh_rotates_and_the_old_token_stops_working() {
    let app = anth_panel::router(state("hunter2"));
    let login = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/auth/login",
            serde_json::json!({ "username": "admin", "password": "hunter2" }),
        ))
        .await
        .unwrap();
    let refresh = cookie_value(&cookie_header(&login, "anth_refresh").unwrap());

    let rotated = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/refresh")
                .header(header::COOKIE, format!("anth_refresh={refresh}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(rotated.status(), StatusCode::OK);
    let new_refresh = cookie_value(&cookie_header(&rotated, "anth_refresh").unwrap());
    assert_ne!(new_refresh, refresh);

    let replay = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/refresh")
                .header(header::COOKIE, format!("anth_refresh={refresh}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(replay.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn logout_clears_the_cookies_and_the_session() {
    let state = state("hunter2");
    let app = anth_panel::router(state.clone());
    let login = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/auth/login",
            serde_json::json!({ "username": "admin", "password": "hunter2" }),
        ))
        .await
        .unwrap();
    let refresh = cookie_value(&cookie_header(&login, "anth_refresh").unwrap());

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/logout")
                .header(header::COOKIE, format!("anth_refresh={refresh}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let cleared = cookie_header(&response, "anth_access").unwrap();
    assert!(cleared.contains("Max-Age=0"));
    assert_eq!(state.auth().await.session_count(), 0);
}

#[tokio::test]
async fn the_session_endpoint_reports_the_login_state() {
    let app = anth_panel::router(state("hunter2"));
    let response = app
        .clone()
        .oneshot(request("/api/auth/session"))
        .await
        .unwrap();
    let body = body_json(response).await;
    assert_eq!(body["enabled"], true);
    assert_eq!(body["authenticated"], false);

    let login = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/auth/login",
            serde_json::json!({ "username": "admin", "password": "hunter2" }),
        ))
        .await
        .unwrap();
    let access = cookie_value(&cookie_header(&login, "anth_access").unwrap());
    let refresh = cookie_value(&cookie_header(&login, "anth_refresh").unwrap());

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/auth/session")
                .header(
                    header::COOKIE,
                    format!("anth_access={access}; anth_refresh={refresh}"),
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = body_json(response).await;
    assert_eq!(body["authenticated"], true);
    assert_eq!(body["username"], "admin");
}

#[tokio::test]
async fn the_session_endpoint_never_leaks_a_secret() {
    let app = anth_panel::router(state("hunter2"));
    let response = app.oneshot(request("/api/auth/session")).await.unwrap();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8_lossy(&bytes).to_string();
    assert!(!text.contains("pbkdf2"));
    assert!(!text.contains("test-secret"));
    assert!(!text.contains("hunter2"));
}

#[tokio::test]
async fn changing_the_password_requires_the_current_one() {
    let app = anth_panel::router(state("hunter2"));
    let access = login_access_cookie(&app).await;
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/password")
                .header(header::COOKIE, format!("anth_access={access}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "current_password": "wrong",
                        "new_password": "a-longer-one"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn changing_the_password_revokes_every_session() {
    let state = state("hunter2");
    let app = anth_panel::router(state.clone());
    let access = login_access_cookie(&app).await;
    assert_eq!(state.auth().await.session_count(), 1);

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/password")
                .header(header::COOKIE, format!("anth_access={access}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "current_password": "hunter2",
                        "new_password": "a-longer-one"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(state.auth().await.session_count(), 0);
    assert!(
        state.auth().await.login("admin", "a-longer-one").is_ok(),
        "the new password must work"
    );
}

#[tokio::test]
async fn a_short_new_password_is_refused() {
    let app = anth_panel::router(state("hunter2"));
    let access = login_access_cookie(&app).await;
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/password")
                .header(header::COOKIE, format!("anth_access={access}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "current_password": "hunter2",
                        "new_password": "short"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn the_config_view_is_protected() {
    let app = anth_panel::router(state("hunter2"));
    let response = app.oneshot(request("/api/config")).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_config_view_never_carries_a_secret() {
    let app = anth_panel::router(state("hunter2"));
    let access = login_access_cookie(&app).await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/config")
                .header(header::COOKIE, format!("anth_access={access}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["auth"]["password_set"], true);
    assert!(body["auth"].get("password_hash").is_none());
    assert!(body["auth"].get("jwt_secret").is_none());
}

#[tokio::test]
async fn patching_the_config_updates_memory_and_the_file() {
    let dir = std::env::temp_dir().join(format!(
        "anth-auth-api-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");

    let config = config_with_auth("hunter2");
    config.save(&path).unwrap();
    let state = AppState::with_config_path(config, Arc::from(path.as_path()));
    let app = anth_panel::router(state.clone());
    let access = login_access_cookie(&app).await;

    let response = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/api/config")
                .header(header::COOKIE, format!("anth_access={access}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"files":{"write":true}}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert!(state.config().await.files.write);
    assert!(
        Config::load(&path).unwrap().files.write,
        "the change must reach the file"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn disabling_auth_from_the_panel_is_refused() {
    let app = anth_panel::router(state("hunter2"));
    let access = login_access_cookie(&app).await;

    let response = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/api/config")
                .header(header::COOKIE, format!("anth_access={access}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"auth":{"enabled":false}}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
