//! The official panel.
//!
//! It links the SDK, which links the engine, and mounts the engine's `/api`
//! router in-process (`D-Remote-Impl`). The browser speaks to one process: the
//! panel serves the shell at `/`, the engine answers `/api/*`, and the Unix
//! socket remains bound for the CLI.
//!
//! A release build embeds `panel/frontend/dist`; a debug build reads it from
//! disk so `bun run build` is enough to refresh the shell.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use anth_panel::{Host, ACTION, SETTINGS, VIEW};
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get};
use axum::Router;
use base64::Engine as _;
use percent_encoding::percent_decode_str;
use subtle::ConstantTimeEq;
use tokio::net::TcpListener;

/// The slots this panel implements, from the SDK rather than a local copy.
const SLOTS: [&str; 3] = [VIEW, ACTION, SETTINGS];

const PANEL_CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
     connect-src 'self'; img-src 'self' data:; font-src 'self'; frame-src 'self'; \
     object-src 'none'; base-uri 'none'; frame-ancestors 'none'";

/// The sandboxed frame document. Its origin is opaque (no `allow-same-origin`),
/// so inline bootstrap and blob imports are contained; network is off.
const FRAME_CSP: &str = "default-src 'none'; script-src 'unsafe-inline' blob:; \
                         style-src 'unsafe-inline'; img-src data:; connect-src 'none'; \
                         base-uri 'none'; frame-ancestors 'self'";

#[derive(Clone)]
struct Panel {
    slots: &'static [&'static str],
    /// The Basic credential expected on `/api/*`, when the panel is exposed.
    authorization: Option<HeaderValue>,
    /// Only the debug build reads from disk; the release build embeds the dist.
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    web: Option<PathBuf>,
}

struct Args {
    listen: SocketAddr,
    config: PathBuf,
    token: Option<String>,
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    web: PathBuf,
}

impl Args {
    fn parse(mut args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut listen = "127.0.0.1:8090".to_string();
        let mut config = "config.toml".to_string();
        let mut token = std::env::var("ANTH_PANEL_TOKEN").ok();
        let mut web = std::env::var("ANTH_PANEL_WEB").unwrap_or_else(|_| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../frontend/dist")
                .display()
                .to_string()
        });
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--listen" => listen = args.next().ok_or("`--listen` needs an address")?,
                "--config" => config = args.next().ok_or("`--config` needs a path")?,
                "--token" => token = Some(args.next().ok_or("`--token` needs a secret")?),
                "--web" => web = args.next().ok_or("`--web` needs a directory")?,
                "--help" | "-h" => {
                    return Err(
                        "usage: anth-panel [--listen 127.0.0.1:8090] [--config <path>] \
                         [--token <secret>] [--web <dir>]"
                            .to_string(),
                    );
                }
                other => return Err(format!("unknown argument `{other}`")),
            }
        }
        Ok(Self {
            listen: listen
                .parse()
                .map_err(|_| format!("`{listen}` is not a socket address"))?,
            config: PathBuf::from(config),
            token,
            web: PathBuf::from(web),
        })
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = match Args::parse(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };

    #[cfg(debug_assertions)]
    if !args.web.join("index.html").is_file() {
        eprintln!(
            "the frontend is not built: {} is missing; run `bun run build` in panel/frontend",
            args.web.display()
        );
        return ExitCode::from(2);
    }

    if args.token.is_none() && !args.listen.ip().is_loopback() {
        eprintln!(
            "refusing to listen on {} without --token: the engine API is privileged; \
             pass a secret or bind 127.0.0.1",
            args.listen
        );
        return ExitCode::from(2);
    }
    if args.token.is_some() {
        eprintln!("anth-panel requires Basic auth on /api (username `operator`)");
    }

    let listener = match TcpListener::bind(args.listen).await {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("binding {}: {error}", args.listen);
            return ExitCode::from(1);
        }
    };

    let host = match Host::start(&args.config).await {
        Ok(host) => host,
        Err(error) => {
            eprintln!("{}: {}", error.kind().code(), error.message());
            return ExitCode::from(error.exit_code());
        }
    };

    let state = Arc::new(Panel {
        slots: &SLOTS,
        authorization: args.token.as_deref().map(basic_authorization),
        #[cfg(debug_assertions)]
        web: Some(args.web.clone()),
        #[cfg(not(debug_assertions))]
        web: None,
    });

    let shell = Router::new()
        .route("/health", get(health))
        .route("/panel/config.json", get(panel_config))
        .fallback(any(asset))
        .with_state(state.clone());
    let api = host
        .api()
        .layer(middleware::from_fn_with_state(state, require_auth));
    let app = shell.nest("/api", api);

    println!(
        "anth-panel listening on http://{} (config {})",
        args.listen,
        args.config.display()
    );
    let served = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown())
        .await;
    host.stop().await;
    match served {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("serving the panel: {error}");
            ExitCode::from(1)
        }
    }
}

/// Liveness for the panel process itself. The engine's readiness is
/// `/api/health`, mounted from the router.
async fn health() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/json")],
        r#"{"status":"ok"}"#,
    )
}

/// The slots this panel implements, so the browser does not hardcode what the
/// SDK already knows.
async fn panel_config(State(state): State<Arc<Panel>>) -> impl IntoResponse {
    let slots = state
        .slots
        .iter()
        .map(|slot| format!("\"{slot}\""))
        .collect::<Vec<_>>()
        .join(",");
    let body = format!(
        r#"{{"version":"{}","slots":[{slots}]}}"#,
        env!("CARGO_PKG_VERSION")
    );
    ([(header::CONTENT_TYPE, "application/json")], body)
}

async fn asset(State(state): State<Arc<Panel>>, request: Request) -> Response {
    if request.method() != Method::GET && request.method() != Method::HEAD {
        return (
            StatusCode::METHOD_NOT_ALLOWED,
            [(header::ALLOW, "GET, HEAD")],
            "method not allowed",
        )
            .into_response();
    }
    let path = request.uri().path();
    let relative = path.trim_start_matches('/');
    let relative = if relative.is_empty() {
        "index.html"
    } else {
        relative
    };
    let relative = match percent_decode_str(relative).decode_utf8() {
        Ok(relative) => relative,
        Err(_) => return not_found(),
    };
    // Browsers still probe the well-known name even when the page declares an
    // SVG icon; answer it with the icon instead of a 404 in the console.
    let relative = if relative == "favicon.ico" {
        "favicon.svg".to_string()
    } else {
        relative.to_string()
    };
    if relative
        .split('/')
        .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return not_found();
    }

    let loaded = load(&state, &relative).await;
    let Some((mime, bytes)) = loaded else {
        return not_found();
    };
    let csp = if relative == "app-frame.html" {
        FRAME_CSP
    } else {
        PANEL_CSP
    };
    let head = request.method() == Method::HEAD;
    let length = bytes.len();
    let body = if head {
        Body::empty()
    } else {
        Body::from(bytes)
    };
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, mime.as_str()),
            (header::CACHE_CONTROL, "no-cache"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (header::CONTENT_SECURITY_POLICY, csp),
            (header::REFERRER_POLICY, "no-referrer"),
        ],
        [(header::CONTENT_LENGTH, length.to_string())],
        body,
    )
        .into_response()
}

/// The engine API is privileged. A loopback panel relies on the OS user; any
/// other bind needs Basic credentials, and a generated token is printed once so
/// the operator can reach it. Sessions replace this shared secret later.
async fn require_auth(State(state): State<Arc<Panel>>, request: Request, next: Next) -> Response {
    if let Some(expected) = &state.authorization {
        let provided = request.headers().get(header::AUTHORIZATION);
        let matches = provided
            .map(|value| bool::from(value.as_bytes().ct_eq(expected.as_bytes())))
            .unwrap_or(false);
        if !matches {
            return (
                StatusCode::UNAUTHORIZED,
                [(
                    header::WWW_AUTHENTICATE,
                    "Basic realm=\"Anthracite\", charset=\"UTF-8\"",
                )],
                "authentication required",
            )
                .into_response();
        }
    }
    next.run(request).await
}

fn basic_authorization(token: &str) -> HeaderValue {
    let encoded = base64::engine::general_purpose::STANDARD.encode(format!("operator:{token}"));
    HeaderValue::from_str(&format!("Basic {encoded}")).expect("a Basic header is valid")
}

#[cfg(debug_assertions)]
async fn load(state: &Panel, relative: &str) -> Option<(String, Vec<u8>)> {
    let root = state.web.as_ref()?;
    let path = root.join(relative);
    let bytes = tokio::fs::read(&path).await.ok()?;
    let mime = mime_guess::from_path(&path)
        .first_or_octet_stream()
        .to_string();
    Some((mime, bytes))
}

#[cfg(not(debug_assertions))]
async fn load(_state: &Panel, relative: &str) -> Option<(String, Vec<u8>)> {
    let file = Frontend::get(relative)?;
    let mime = mime_guess::from_path(relative)
        .first_or_octet_stream()
        .to_string();
    Some((mime, file.data.into_owned()))
}

#[cfg(not(debug_assertions))]
#[derive(rust_embed::RustEmbed)]
#[folder = "../frontend/dist"]
struct Frontend;

fn not_found() -> Response {
    envelope(StatusCode::NOT_FOUND, "not-found", "no such panel asset")
}

fn envelope(status: StatusCode, code: &str, message: &str) -> Response {
    let body = format!(r#"{{"error":{{"code":"{code}","message":"{message}"}}}}"#);
    (status, [(header::CONTENT_TYPE, "application/json")], body).into_response()
}

/// Ctrl-C or SIGTERM, whichever comes first.
async fn shutdown() {
    let mut terminate =
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(signal) => signal,
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
                return;
            }
        };
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = terminate.recv() => {}
    }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use axum::body::to_bytes;

    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir()
            .join("anthracite-panel")
            .join(format!("{name}-{}-{stamp}", std::process::id()))
    }

    fn panel(web: Option<PathBuf>) -> Arc<Panel> {
        Arc::new(Panel {
            slots: &SLOTS,
            authorization: None,
            web,
        })
    }

    #[test]
    fn the_default_listen_is_loopback() {
        let args = Args::parse(std::iter::empty()).expect("args");
        assert_eq!(args.listen, "127.0.0.1:8090".parse().expect("address"));
    }

    #[test]
    fn basic_auth_uses_the_operator_name_and_token() {
        assert_eq!(
            basic_authorization("secret").to_str().expect("header"),
            "Basic b3BlcmF0b3I6c2VjcmV0"
        );
    }

    #[test]
    fn the_slots_come_from_the_sdk() {
        assert_eq!(
            SLOTS,
            [anth_panel::VIEW, anth_panel::ACTION, anth_panel::SETTINGS]
        );
    }

    #[cfg(debug_assertions)]
    #[tokio::test]
    async fn the_static_shell_is_served_with_security_headers() {
        let dir = scratch("static");
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(dir.join("index.html"), "<h1>panel</h1>").expect("index");
        std::fs::write(dir.join("main.js"), "export {};").expect("js");
        std::fs::write(dir.join("favicon.svg"), "<svg/>").expect("icon");
        let state = panel(Some(dir.clone()));

        let response = asset(
            State(state.clone()),
            Request::builder()
                .uri("/")
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "text/html");
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-cache");
        assert!(response.headers()[header::CONTENT_SECURITY_POLICY]
            .to_str()
            .expect("csp")
            .contains("default-src 'self'"));
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        assert_eq!(&body[..], b"<h1>panel</h1>");

        let head = asset(
            State(state.clone()),
            Request::builder()
                .method("HEAD")
                .uri("/main.js")
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(head.status(), StatusCode::OK);
        assert_eq!(head.headers()[header::CONTENT_LENGTH], "10");
        let body = to_bytes(head.into_body(), usize::MAX).await.expect("body");
        assert!(body.is_empty());

        let icon = asset(
            State(state.clone()),
            Request::builder()
                .uri("/favicon.ico")
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(icon.status(), StatusCode::OK);
        assert_eq!(icon.headers()[header::CONTENT_TYPE], "image/svg+xml");

        for path in ["/../secret", "/%2e%2e/secret", "/missing.js"] {
            let response = asset(
                State(state.clone()),
                Request::builder()
                    .uri(path)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
        }

        let rejected = asset(
            State(state),
            Request::builder()
                .method("POST")
                .uri("/")
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(rejected.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(rejected.headers()[header::ALLOW], "GET, HEAD");
        let _ = std::fs::remove_dir_all(dir);
    }
}
