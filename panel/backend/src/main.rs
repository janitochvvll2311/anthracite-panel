//! The official panel.
//!
//! It serves the built frontend and forwards `/api/*` to the engine over the
//! Unix socket, unchanged: method, body and headers. The panel is a client of
//! the engine, not a second router, which is why this crate never depends on
//! `engine/*`. The browser sees one origin, so there is no CORS.
//!
//! A release build embeds `panel/frontend/dist`; a debug build reads it from
//! disk so `bun run build` is enough to refresh the shell.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{header, HeaderName, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get};
use axum::Router;
use http_body_util::Full;
use hyper::client::conn::http1;
use hyper_util::rt::TokioIo;
use percent_encoding::percent_decode_str;
use tokio::net::{TcpListener, UnixStream};

/// The install cap is 32 MiB; the extra room is the JSON/headers a request
/// carries around the bundle.
const MAX_BODY: usize = 34 * 1024 * 1024;

/// The slots this panel implements, from the SDK rather than a local copy.
const SLOTS: [&str; 3] = [anth_panel::VIEW, anth_panel::ACTION, anth_panel::SETTINGS];

const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
                   connect-src 'self'; img-src 'self' data:; font-src 'self'; object-src 'none'; \
                   base-uri 'none'; frame-ancestors 'none'";

#[derive(Clone)]
struct Panel {
    engine: PathBuf,
    slots: &'static [&'static str],
    /// Only the debug build reads from disk; the release build embeds the dist.
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    web: Option<PathBuf>,
}

struct Args {
    listen: SocketAddr,
    engine: PathBuf,
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    web: PathBuf,
}

impl Args {
    fn parse(mut args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut listen = "0.0.0.0:8090".to_string();
        let mut engine = std::env::var("ANTH_ENGINE_SOCKET")
            .unwrap_or_else(|_| "/run/anth/engine.sock".to_string());
        let mut web = std::env::var("ANTH_PANEL_WEB").unwrap_or_else(|_| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../frontend/dist")
                .display()
                .to_string()
        });
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--listen" => listen = args.next().ok_or("`--listen` needs an address")?,
                "--engine" => engine = args.next().ok_or("`--engine` needs a socket path")?,
                "--web" => web = args.next().ok_or("`--web` needs a directory")?,
                "--help" | "-h" => {
                    return Err(
                        "usage: anth-panel [--listen 0.0.0.0:8090] [--engine <socket>] [--web <dir>]"
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
            engine: PathBuf::from(engine),
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

    let listener = match TcpListener::bind(args.listen).await {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("binding {}: {error}", args.listen);
            return ExitCode::from(1);
        }
    };

    let state = Arc::new(Panel {
        engine: args.engine.clone(),
        slots: &SLOTS,
        #[cfg(debug_assertions)]
        web: Some(args.web.clone()),
        #[cfg(not(debug_assertions))]
        web: None,
    });

    let app = Router::new()
        .route("/health", get(health))
        .route("/panel/config.json", get(panel_config))
        .route("/api", any(proxy))
        .route("/api/{*path}", any(proxy))
        .fallback(any(asset))
        .with_state(state);

    println!(
        "anth-panel listening on http://{} (engine {})",
        args.listen,
        args.engine.display()
    );
    match axum::serve(listener, app)
        .with_graceful_shutdown(shutdown())
        .await
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("serving the panel: {error}");
            ExitCode::from(1)
        }
    }
}

/// Liveness for the panel process itself. It says nothing about the engine.
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

/// Forwards a browser request to the engine over the Unix socket.
///
/// The engine sees the panel as its own peer, which is how a local panel
/// authenticates before sessions exist. Hop-by-hop headers stay on this side;
/// everything else is passed through unchanged.
async fn proxy(State(state): State<Arc<Panel>>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let bytes = match axum::body::to_bytes(body, MAX_BODY).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return envelope(
                StatusCode::PAYLOAD_TOO_LARGE,
                "too-large",
                "the request body is larger than the panel proxy accepts",
            )
        }
    };

    let stream = match UnixStream::connect(&state.engine).await {
        Ok(stream) => stream,
        Err(error) => {
            eprintln!("connecting to {}: {error}", state.engine.display());
            return envelope(
                StatusCode::BAD_GATEWAY,
                "unavailable",
                "the engine is not reachable",
            );
        }
    };

    let (mut sender, connection) = match http1::handshake(TokioIo::new(stream)).await {
        Ok(pair) => pair,
        Err(error) => {
            eprintln!("engine handshake: {error}");
            return envelope(
                StatusCode::BAD_GATEWAY,
                "unavailable",
                "the engine refused the connection",
            );
        }
    };
    tokio::spawn(connection);

    let target = parts
        .uri
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/");
    let mut builder = hyper::Request::builder()
        .method(parts.method.clone())
        .uri(target);
    for (name, value) in &parts.headers {
        if !forward_request_header(name) {
            continue;
        }
        builder = builder.header(name, value);
    }
    let outgoing = match builder.body(Full::new(bytes)) {
        Ok(request) => request,
        Err(error) => {
            eprintln!("building the proxied request: {error}");
            return envelope(
                StatusCode::BAD_REQUEST,
                "invalid",
                "the request cannot be forwarded",
            );
        }
    };

    let response = match sender.send_request(outgoing).await {
        Ok(response) => response,
        Err(error) => {
            eprintln!("engine request: {error}");
            return envelope(
                StatusCode::BAD_GATEWAY,
                "unavailable",
                "the engine did not answer",
            );
        }
    };

    let (head, body) = response.into_parts();
    let mut response = Response::new(Body::new(body));
    *response.status_mut() = head.status;
    for (name, value) in &head.headers {
        if forward_response_header(name) {
            response.headers_mut().append(name.clone(), value.clone());
        }
    }
    response
}

fn forward_request_header(name: &HeaderName) -> bool {
    let name = name.as_str();
    !name.starts_with("proxy-")
        && !matches!(
            name,
            "host"
                | "content-length"
                | "connection"
                | "keep-alive"
                | "transfer-encoding"
                | "te"
                | "trailer"
                | "upgrade"
        )
}

fn forward_response_header(name: &HeaderName) -> bool {
    let name = name.as_str();
    !name.starts_with("proxy-")
        && !matches!(
            name,
            "connection" | "keep-alive" | "transfer-encoding" | "te" | "trailer" | "upgrade"
        )
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
            (header::CONTENT_SECURITY_POLICY, CSP),
            (header::REFERRER_POLICY, "no-referrer"),
        ],
        [(header::CONTENT_LENGTH, length.to_string())],
        body,
    )
        .into_response()
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
    use std::convert::Infallible;
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    use axum::body::{to_bytes, Bytes};
    use http_body_util::BodyExt;
    use hyper::service::service_fn;

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

    fn panel(engine: PathBuf, web: Option<PathBuf>) -> Arc<Panel> {
        Arc::new(Panel {
            engine,
            slots: &SLOTS,
            web,
        })
    }

    #[test]
    fn the_default_listen_is_all_interfaces() {
        let args = Args::parse(std::iter::empty()).expect("args");
        assert_eq!(args.listen, "0.0.0.0:8090".parse().expect("address"));
    }

    #[test]
    fn hop_by_hop_headers_do_not_cross_the_proxy() {
        assert!(!forward_request_header(&header::CONNECTION));
        assert!(!forward_request_header(&HeaderName::from_static(
            "transfer-encoding"
        )));
        assert!(forward_request_header(&header::ACCEPT));
        assert!(forward_request_header(&HeaderName::from_static(
            "if-none-match"
        )));
    }

    #[test]
    fn the_slots_come_from_the_sdk() {
        assert_eq!(
            SLOTS,
            [anth_panel::VIEW, anth_panel::ACTION, anth_panel::SETTINGS]
        );
    }

    #[derive(Debug)]
    struct Captured {
        method: String,
        target: String,
        test: Option<String>,
        host: bool,
        connection: bool,
        body: Vec<u8>,
    }

    #[tokio::test]
    async fn the_proxy_preserves_method_body_and_headers() {
        let dir = scratch("proxy");
        std::fs::create_dir_all(&dir).expect("mkdir");
        let socket = dir.join("engine.sock");
        let listener = tokio::net::UnixListener::bind(&socket).expect("bind");
        let captured = Arc::new(Mutex::new(None::<Captured>));
        let seen = captured.clone();

        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept");
            let _ = hyper::server::conn::http1::Builder::new()
                .serve_connection(
                    TokioIo::new(stream),
                    service_fn(move |request: hyper::Request<hyper::body::Incoming>| {
                        let seen = seen.clone();
                        async move {
                            let (parts, body) = request.into_parts();
                            let body = body.collect().await.expect("body").to_bytes();
                            *seen.lock().expect("lock") = Some(Captured {
                                method: parts.method.to_string(),
                                target: parts.uri.to_string(),
                                test: parts
                                    .headers
                                    .get("x-test")
                                    .and_then(|value| value.to_str().ok())
                                    .map(str::to_string),
                                host: parts.headers.contains_key("host"),
                                connection: parts.headers.contains_key("connection"),
                                body: body.to_vec(),
                            });
                            Ok::<_, Infallible>(
                                hyper::Response::builder()
                                    .status(StatusCode::CREATED)
                                    .header("x-engine", "yes")
                                    .body(Full::new(Bytes::from_static(b"created")))
                                    .expect("response"),
                            )
                        }
                    }),
                )
                .await;
        });

        let request = Request::builder()
            .method("POST")
            .uri("/api/engine/apps?x=1")
            .header("x-test", "yes")
            .header("host", "browser.example")
            .header("connection", "close")
            .body(Body::from("hello"))
            .expect("request");
        let response = proxy(State(panel(socket, None)), request).await;
        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(response.headers()["x-engine"], "yes");
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        assert_eq!(&body[..], b"created");

        let captured = captured
            .lock()
            .expect("lock")
            .take()
            .expect("the engine saw the request");
        assert_eq!(captured.method, "POST");
        assert_eq!(captured.target, "/api/engine/apps?x=1");
        assert_eq!(captured.test.as_deref(), Some("yes"));
        assert!(!captured.host, "the browser's Host is not the engine's");
        assert!(!captured.connection, "connection is hop-by-hop");
        assert_eq!(captured.body, b"hello");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn an_unreachable_engine_is_a_gateway_error() {
        let request = Request::builder()
            .uri("/api/health")
            .body(Body::empty())
            .expect("request");
        let response = proxy(
            State(panel(
                PathBuf::from("/nonexistent/anthracite-panel/engine.sock"),
                None,
            )),
            request,
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    }

    #[cfg(debug_assertions)]
    #[tokio::test]
    async fn the_static_shell_is_served_with_security_headers() {
        let dir = scratch("static");
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(dir.join("index.html"), "<h1>panel</h1>").expect("index");
        std::fs::write(dir.join("main.js"), "export {};").expect("js");
        std::fs::write(dir.join("favicon.svg"), "<svg/>").expect("icon");
        let state = panel(PathBuf::new(), Some(dir.clone()));

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
