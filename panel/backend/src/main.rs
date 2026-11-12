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

use anth_panel::{Host, Sessions, ACTION, FAB, MODAL, SETTINGS, VIEW};
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{header, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get};
use axum::Router;
use percent_encoding::percent_decode_str;
use tokio::net::TcpListener;

/// The slots this panel implements, from the SDK rather than a local copy.
const SLOTS: [&str; 5] = [VIEW, ACTION, SETTINGS, FAB, MODAL];

const PANEL_CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
     connect-src 'self'; img-src 'self' data:; font-src 'self'; frame-src 'self'; \
     object-src 'none'; base-uri 'none'; frame-ancestors 'none'";

/// The sandboxed frame document. Its origin is opaque (no `allow-same-origin`),
/// so inline bootstrap and blob imports are contained; network is off.
const FRAME_CSP: &str = "default-src 'none'; script-src 'unsafe-inline' blob:; \
                         style-src 'unsafe-inline'; img-src data: blob:; connect-src 'none'; \
                         base-uri 'none'; frame-ancestors 'self'";

/// The one page that needs an inline script. It carries no user content: a form
/// and a fetch to the operator resource.
const SETUP_CSP: &str = "default-src 'self'; script-src 'unsafe-inline'; \
                         style-src 'unsafe-inline'; connect-src 'self'; object-src 'none'; \
                         base-uri 'none'; frame-ancestors 'none'";

#[derive(Clone)]
struct Panel {
    slots: &'static [&'static str],
    /// The engine's session store, shared with the mounted API router.
    sessions: Option<Sessions>,
    /// Only the debug build reads from disk; the release build embeds the dist.
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    web: Option<PathBuf>,
}

struct Args {
    listen: SocketAddr,
    config: PathBuf,
    allow_remote: bool,
    reset_credentials: bool,
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    web: PathBuf,
}

impl Args {
    fn parse(mut args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut listen = "127.0.0.1:8090".to_string();
        let mut config = "config.toml".to_string();
        let mut allow_remote = false;
        let mut reset_credentials = false;
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
                "--allow-remote" => allow_remote = true,
                "--reset-credentials" => reset_credentials = true,
                "--web" => web = args.next().ok_or("`--web` needs a directory")?,
                "--help" | "-h" => {
                    return Err(
                        "usage: anth-panel [--listen 127.0.0.1:8090] [--config <path>] \
                         [--allow-remote] [--reset-credentials] [--web <dir>]"
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
            allow_remote,
            reset_credentials,
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

    if !args.listen.ip().is_loopback() && !args.allow_remote {
        eprintln!(
            "refusing to listen on {} without --allow-remote: sessions protect the \
             API, but exposing the panel is an explicit decision",
            args.listen
        );
        return ExitCode::from(2);
    }
    if !args.listen.ip().is_loopback() {
        eprintln!(
            "warning: listening on {}; put TLS in front of the panel and set \
             X-Forwarded-Proto/Host",
            args.listen
        );
    }

    let listener = match TcpListener::bind(args.listen).await {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("binding {}: {error}", args.listen);
            return ExitCode::from(1);
        }
    };

    let host = match Host::start_with_reset(&args.config, args.reset_credentials).await {
        Ok(host) => host,
        Err(error) => {
            eprintln!("{}: {}", error.kind().code(), error.message());
            return ExitCode::from(error.exit_code());
        }
    };

    let state = Arc::new(Panel {
        slots: &SLOTS,
        sessions: host.sessions(),
        #[cfg(debug_assertions)]
        web: Some(args.web.clone()),
        #[cfg(not(debug_assertions))]
        web: None,
    });

    let shell = Router::new()
        .route("/health", get(health))
        .route("/panel/config.json", get(panel_config))
        .route("/login", get(login_page))
        .route("/setup", get(setup))
        .fallback(any(asset))
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(
            Arc::clone(&state),
            require_shell_session,
        ));
    // `/api` authenticates itself with the engine's session cookie; the shell
    // gate above only applies to the panel's own documents and assets.
    let app = shell.nest("/api", host.session_api());

    println!(
        "anth-panel listening on http://{} (config {})",
        args.listen,
        args.config.display()
    );
    let served = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
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

/// The login page. It is public; the engine mints the session cookie and the
/// shell gate reads it on the next navigation.
async fn login_page() -> Response {
    page(
        "Sign in · Anthracite",
        r#"<div class="copy">
  <h1>Welcome back</h1>
  <p>Sign in as <code>operator</code> to open the workspace.</p>
</div>
<form id="login" class="form">
  <label class="field">
    <span>Password</span>
    <input id="password" type="password" autocomplete="current-password" placeholder="Operator password" autofocus required>
  </label>
  <button class="primary" type="submit">Sign in</button>
  <p id="status" class="status" role="status" aria-live="polite"></p>
</form>
<p class="hint">First boot? Read the password from <code>state/credentials.txt</code>.</p>
<script>
document.getElementById('login').addEventListener('submit', async (event) => {
  event.preventDefault();
  const status = document.getElementById('status');
  status.textContent = '';
  status.dataset.state = '';
  const response = await fetch('/api/engine/session', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ operator: 'operator', password: document.getElementById('password').value }),
  });
  if (!response.ok) {
    status.dataset.state = 'bad';
    status.textContent = await response.text();
    return;
  }
  const session = await response.json();
  location.href = session.must_change_password ? '/setup' : '/';
});
</script>"#,
    )
}

/// The first-boot escape hatch. It logs in with the generated password, then
/// changes it; the engine gate refuses every other resource until then.
async fn setup() -> Response {
    page(
        "Set a password · Anthracite",
        r#"<div class="copy">
  <h1>Set a new password</h1>
  <p>Replace the generated operator password before opening the workspace.</p>
</div>
<form id="setup" class="form">
  <label class="field">
    <span>Current password</span>
    <input id="current" type="password" autocomplete="current-password" placeholder="From state/credentials.txt" autofocus required>
  </label>
  <label class="field">
    <span>New password</span>
    <input id="new" type="password" autocomplete="new-password" placeholder="At least 12 characters" minlength="12" required>
  </label>
  <button class="primary" type="submit">Change password</button>
  <p id="status" class="status" role="status" aria-live="polite"></p>
</form>
<p class="hint">The current password is in <code>state/credentials.txt</code>. The engine refuses every other resource until it changes.</p>
<script>
document.getElementById('setup').addEventListener('submit', async (event) => {
  event.preventDefault();
  const status = document.getElementById('status');
  status.textContent = '';
  status.dataset.state = '';
  const current = document.getElementById('current').value;
  const next = document.getElementById('new').value;
  const login = await fetch('/api/engine/session', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ operator: 'operator', password: current }),
  });
  if (!login.ok) {
    status.dataset.state = 'bad';
    status.textContent = await login.text();
    return;
  }
  const response = await fetch('/api/engine/operator', {
    method: 'PATCH',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ current_password: current, new_password: next }),
  });
  if (!response.ok) {
    status.dataset.state = 'bad';
    status.textContent = await response.text();
    return;
  }
  status.dataset.state = 'ok';
  status.textContent = 'Password changed. Opening the workspace…';
  location.href = '/';
});
</script>"#,
    )
}

fn page(title: &str, body: &str) -> Response {
    let version = env!("CARGO_PKG_VERSION");
    let html = format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="color-scheme" content="dark">
<title>{title}</title>
<style>
  /* Mirrors panel/frontend/src/style.css; /login is public and cannot load
     the gated shell CSS. */
  :root {{
    color-scheme: dark;
    --bg: #0b0d12;
    --bg-soft: #0f1219;
    --panel: #141821;
    --line: #2a3142;
    --line-soft: #202634;
    --text: #e8ebf2;
    --muted: #8f98ab;
    --muted-2: #667087;
    --accent: #7aa2ff;
    --accent-2: #9d7bff;
    --danger: #ff7d8a;
    --good: #52d1a4;
    --shadow: 0 24px 70px #00000073;
    font-synthesis: none;
  }}
  * {{ box-sizing: border-box; }}
  html, body {{ min-height: 100%; }}
  body {{
    margin: 0;
    min-height: 100vh;
    display: grid;
    place-items: center;
    padding: 2rem 1rem;
    background:
      radial-gradient(900px 420px at 12% -10%, #1b2540 0%, transparent 62%),
      radial-gradient(760px 420px at 96% 0%, #241b3d 0%, transparent 58%),
      var(--bg);
    color: var(--text);
    font: 14px/1.45 ui-sans-serif, system-ui, -apple-system, "Segoe UI", Roboto, sans-serif;
  }}
  .auth {{ width: min(420px, 100%); display: grid; gap: 1rem; }}
  .card {{
    padding: 2rem;
    border: 1px solid var(--line);
    border-radius: 16px;
    background: linear-gradient(180deg, #171b24, #12151c);
    box-shadow: var(--shadow);
  }}
  .brand {{
    display: flex;
    align-items: center;
    gap: 10px;
    margin-bottom: 1.6rem;
    font-size: 15px;
    font-weight: 650;
    letter-spacing: .01em;
  }}
  .brand-mark {{
    width: 26px;
    height: 26px;
    border-radius: 8px;
    background: linear-gradient(135deg, var(--accent) 0%, var(--accent-2) 62%, #f08fb1 100%);
    box-shadow: 0 0 22px #7aa2ff59;
    position: relative;
  }}
  .brand-mark::after {{
    content: "";
    position: absolute;
    inset: 7px;
    border-radius: 3px;
    border: 1.5px solid #ffffffc7;
    border-left-color: transparent;
    border-bottom-color: transparent;
    transform: rotate(45deg);
  }}
  .copy h1 {{
    margin: 0 0 .35rem;
    font-size: 1.55rem;
    line-height: 1.2;
    letter-spacing: -.02em;
    font-weight: 650;
  }}
  .copy p {{ margin: 0 0 1.5rem; color: var(--muted); }}
  .field {{ display: block; margin-bottom: 1rem; }}
  .field span {{
    display: block;
    margin-bottom: .4rem;
    color: var(--muted);
    font-size: .72rem;
    letter-spacing: .09em;
    text-transform: uppercase;
  }}
  input {{
    width: 100%;
    padding: .7rem .8rem;
    color: var(--text);
    background: var(--bg-soft);
    border: 1px solid var(--line);
    border-radius: 9px;
    font: inherit;
    outline: none;
    transition: border-color .15s ease, box-shadow .15s ease;
  }}
  input::placeholder {{ color: var(--muted-2); }}
  input:focus {{
    border-color: var(--accent);
    box-shadow: 0 0 0 3px #7aa2ff22;
  }}
  .primary {{
    width: 100%;
    margin-top: .35rem;
    padding: .72rem 1rem;
    border: 1px solid #8aa9ff55;
    border-radius: 9px;
    background: linear-gradient(135deg, var(--accent), var(--accent-2));
    color: #0b0d12;
    font: inherit;
    font-weight: 650;
    cursor: pointer;
    transition: filter .15s ease, transform .15s ease;
  }}
  .primary:hover {{ filter: brightness(1.06); }}
  .primary:active {{ transform: translateY(1px); }}
  .status {{
    min-height: 1.2em;
    margin: .9rem 0 0;
    color: var(--danger);
    font-size: .82rem;
    white-space: pre-wrap;
  }}
  .status[data-state="ok"] {{ color: var(--good); }}
  .hint {{ margin: 1.25rem 0 0; color: var(--muted); font-size: .8rem; }}
  code {{
    padding: .1rem .3rem;
    border: 1px solid var(--line-soft);
    border-radius: 5px;
    background: #ffffff0d;
    color: var(--muted);
    font-size: .9em;
  }}
  .version {{ margin: 0; color: var(--muted-2); font-size: .75rem; text-align: center; }}
</style>
</head>
<body>
<main class="auth">
  <section class="card">
    <header class="brand"><span class="brand-mark" aria-hidden="true"></span><span>Anthracite</span></header>
    {body}
  </section>
  <p class="version">Anthracite v{version}</p>
</main>
</body>
</html>"#
    );
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CONTENT_SECURITY_POLICY, SETUP_CSP),
            (header::CACHE_CONTROL, "no-cache"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (header::REFERRER_POLICY, "no-referrer"),
            (header::X_FRAME_OPTIONS, "DENY"),
        ],
        html,
    )
        .into_response()
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
            (header::X_FRAME_OPTIONS, "DENY"),
        ],
        [(header::CONTENT_LENGTH, length.to_string())],
        body,
    )
        .into_response()
}

/// The shell is static, but it is not public: an unauthenticated browser goes to
/// `/login`, and a first-boot session goes to `/setup`. The sandboxed frame
/// document and the login pages are the only exceptions.
async fn require_shell_session(
    State(state): State<Arc<Panel>>,
    request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path();
    if matches!(
        path,
        "/health" | "/login" | "/setup" | "/app-frame.html" | "/favicon.ico" | "/panel/config.json"
    ) {
        return next.run(request).await;
    }
    let session = state.sessions.as_ref().and_then(|sessions| {
        anth_panel::session_token(request.headers()).and_then(|token| sessions.authenticate(&token))
    });
    match session {
        Some(session) if session.must_change_password => redirect("/setup"),
        Some(_) => next.run(request).await,
        None => redirect("/login"),
    }
}

fn redirect(location: &str) -> Response {
    (
        StatusCode::SEE_OTHER,
        [(header::LOCATION, location)],
        "redirecting",
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
    use std::time::{SystemTime, UNIX_EPOCH};

    use axum::body::to_bytes;
    use tower::ServiceExt;

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
            sessions: None,
            web,
        })
    }

    #[test]
    fn the_default_listen_is_loopback() {
        let args = Args::parse(std::iter::empty()).expect("args");
        assert_eq!(args.listen, "127.0.0.1:8090".parse().expect("address"));
        assert!(!args.allow_remote);
        let args = Args::parse(
            ["--listen", "0.0.0.0:8090", "--allow-remote"]
                .into_iter()
                .map(str::to_string),
        )
        .expect("args");
        assert!(args.allow_remote);
        assert!(!args.reset_credentials);
        let args =
            Args::parse(["--reset-credentials"].into_iter().map(str::to_string)).expect("args");
        assert!(args.reset_credentials);
    }

    #[test]
    fn the_slots_come_from_the_sdk() {
        assert_eq!(
            SLOTS,
            [
                anth_panel::VIEW,
                anth_panel::ACTION,
                anth_panel::SETTINGS,
                anth_panel::FAB,
                anth_panel::MODAL,
            ]
        );
    }

    #[tokio::test]
    async fn the_login_page_posts_to_the_session_endpoint() {
        let response = login_page().await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let text = String::from_utf8(body.to_vec()).expect("utf8");
        assert!(text.contains("/api/engine/session"), "{text}");
        assert!(text.contains("must_change_password"), "{text}");
        assert!(text.contains("class=\"card\""), "{text}");
        assert!(text.contains("brand-mark"), "{text}");
        assert!(text.contains("Welcome back"), "{text}");
    }

    #[tokio::test]
    async fn the_shell_redirects_an_anonymous_request_to_login() {
        let state = panel(None);
        let app = Router::new()
            .route("/", get(|| async { "shell" }))
            .with_state(state.clone())
            .layer(middleware::from_fn_with_state(state, require_shell_session));
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers()[header::LOCATION], "/login");
    }

    #[tokio::test]
    async fn the_setup_page_carries_the_password_form_and_a_narrow_csp() {
        let response = setup().await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "text/html; charset=utf-8"
        );
        let csp = response.headers()[header::CONTENT_SECURITY_POLICY]
            .to_str()
            .expect("csp");
        assert!(csp.contains("script-src 'unsafe-inline'"), "{csp}");
        assert_eq!(
            response.headers()[header::X_CONTENT_TYPE_OPTIONS],
            "nosniff"
        );
        assert_eq!(response.headers()[header::REFERRER_POLICY], "no-referrer");
        assert_eq!(response.headers()[header::X_FRAME_OPTIONS], "DENY");
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let text = String::from_utf8(body.to_vec()).expect("utf8");
        assert!(text.contains("current_password"), "{text}");
        assert!(text.contains("/api/engine/operator"), "{text}");
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
