//! `anth-demo-sessions`: one panel and one emulator state per browser session.
//!
//! The demo variant that runs a single panel shares one emulator snapshot between
//! every visitor. This launcher gives each browser its own panel on its own port,
//! with its own state directory, so files, users, routes and mounts persist for
//! that session and never leak into another one. It is demo tooling, not a
//! deployment mode: it binds loopback by default, has a session cap and leaves
//! the state directories in place when it exits.

use std::collections::BTreeMap;
use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

const COOKIE: &str = "anth_demo_session";
const APPS: &[&str] = &[
    "accounts", "firewall", "fs", "home", "net", "proc", "storage", "terminal",
];
const HOST_APPS: &[&str] = &["home"];

#[derive(Clone)]
struct Config {
    panel: PathBuf,
    cli: PathBuf,
    bundles: PathBuf,
    state: PathBuf,
    listen: SocketAddr,
    /// The host the browser used, when the launcher cannot infer it.
    public_host: Option<String>,
    max: usize,
}

struct Session {
    port: u16,
    state: PathBuf,
    password: String,
    child: Child,
}

struct App {
    config: Config,
    sessions: Mutex<BTreeMap<String, Session>>,
    create_lock: Mutex<()>,
    next: AtomicU64,
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let config = match parse(std::env::args().skip(1)) {
        Ok(config) => config,
        Err(message) => {
            eprintln!("{message}");
            return std::process::ExitCode::from(2);
        }
    };
    if !config.listen.ip().is_loopback() {
        eprintln!(
            "warning: listening on {}; every child panel will too. Expose this only \
             behind a trusted tunnel or proxy.",
            config.listen
        );
    }
    if let Err(error) = std::fs::create_dir_all(&config.state) {
        eprintln!("creating {}: {error}", config.state.display());
        return std::process::ExitCode::from(1);
    }
    let app = Arc::new(App {
        config: config.clone(),
        sessions: Mutex::new(BTreeMap::new()),
        create_lock: Mutex::new(()),
        next: AtomicU64::new(1),
    });

    let router = Router::new()
        .route("/", get(root))
        .route("/new", get(new_session))
        .route("/health", get(|| async { "ok" }))
        .route("/favicon.ico", get(|| async { StatusCode::NO_CONTENT }))
        .with_state(Arc::clone(&app));

    let listener = match tokio::net::TcpListener::bind(config.listen).await {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("binding {}: {error}", config.listen);
            return std::process::ExitCode::from(1);
        }
    };
    println!(
        "anth-demo-sessions listening on http://{} (state {})",
        config.listen,
        config.state.display()
    );

    let supervisor = Arc::clone(&app);
    tokio::spawn(async move {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).ok();
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = async {
                match terminate.as_mut() {
                    Some(signal) => {
                        signal.recv().await;
                    }
                    None => std::future::pending::<()>().await,
                }
            } => {}
        }
        let _ = shutdown(&supervisor).await;
        std::process::exit(0);
    });

    let served = axum::serve(listener, router).await;
    let _ = shutdown(&app).await;
    match served {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("serving: {error}");
            std::process::ExitCode::from(1)
        }
    }
}

fn parse(args: impl Iterator<Item = String>) -> Result<Config, String> {
    let mut panel = None;
    let mut cli = None;
    let mut bundles = None;
    let mut state = PathBuf::from("state");
    let mut listen: SocketAddr = "127.0.0.1:8091".parse().expect("a valid default address");
    let mut public_host = None;
    let mut max = 8usize;
    let mut args = args.peekable();
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--panel" => panel = args.next().map(PathBuf::from),
            "--cli" => cli = args.next().map(PathBuf::from),
            "--bundles" => bundles = args.next().map(PathBuf::from),
            "--state" => state = args.next().map(PathBuf::from).unwrap_or(state),
            "--listen" => {
                let value = args.next().ok_or("`--listen` needs an address")?;
                listen = value
                    .parse()
                    .map_err(|_| format!("`{value}` is not a socket address"))?;
            }
            "--public-host" => public_host = args.next(),
            "--max" => {
                let value = args.next().ok_or("`--max` needs a number")?;
                max = value
                    .parse()
                    .map_err(|_| "`--max` needs a positive number".to_string())?;
            }
            "--help" | "-h" => {
                println!(
                    "usage: anth-demo-sessions --panel <path> --cli <path> --bundles <dir> \
                     [--state <dir>] [--listen <addr>] [--public-host <host>] [--max <n>]"
                );
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument `{other}`")),
        }
    }
    Ok(Config {
        panel: panel.ok_or("`--panel` is required")?,
        cli: cli.ok_or("`--cli` is required")?,
        bundles: bundles.ok_or("`--bundles` is required")?,
        state,
        listen,
        public_host,
        max,
    })
}

async fn root(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    match cookie_session(&headers) {
        Some(id) => match session_page(&app, &id, &headers).await {
            Some(response) => response,
            None => create_session_page(&app, &headers).await,
        },
        None => create_session_page(&app, &headers).await,
    }
}

async fn new_session(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    create_session_page(&app, &headers).await
}

/// The page a browser lands on: it owns a session and shows the one password it
/// needs. The panel itself is on its own port, so cookies stay per browser and
/// the emulator state stays per session.
async fn session_page(app: &App, id: &str, headers: &HeaderMap) -> Option<Response> {
    let sessions = app.sessions.lock().await;
    let session = sessions.get(id)?;
    let url = format!("http://{}:{}/", host(app, Some(headers)), session.port);
    Some(
        Html(page(
            "Anthracite demo",
            &format!(
                "<p>This browser owns a private demo session: its files, users, routes \
             and mounts persist for this session and are never shared.</p>\
             <p><a href=\"{url}\">Open the panel</a></p>\
             <p>Operator <code>operator</code>, password <code>{}</code>.</p>\
             <p>The state lives in <code>{}</code>.</p>\
             <p><a href=\"/new\">Start a new session</a> (this one keeps its state but \
             the browser cookie moves).</p>",
                escape(&session.password),
                escape(&session.state.display().to_string())
            ),
        ))
        .into_response(),
    )
}

fn cookie_session(headers: &HeaderMap) -> Option<String> {
    let cookies = headers.get(header::COOKIE)?.to_str().ok()?;
    for pair in cookies.split(';') {
        let pair = pair.trim();
        if let Some(value) = pair.strip_prefix(&format!("{COOKIE}=")) {
            if !value.is_empty() && value.len() <= 96 {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn host(app: &App, headers: Option<&HeaderMap>) -> String {
    if let Some(host) = &app.config.public_host {
        return host.clone();
    }
    if let Some(headers) = headers {
        if let Some(value) = headers
            .get(header::HOST)
            .and_then(|value| value.to_str().ok())
        {
            if let Some((host, _)) = value.rsplit_once(':') {
                return host.to_string();
            }
            return value.to_string();
        }
    }
    app.config.listen.ip().to_string()
}

async fn create_session_page(app: &Arc<App>, headers: &HeaderMap) -> Response {
    let _guard = app.create_lock.lock().await;
    {
        let sessions = app.sessions.lock().await;
        if sessions.len() >= app.config.max {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Html(page(
                    "Demo is full",
                    "<p>Every demo session is in use. Try again in a few minutes.</p>",
                )),
            )
                .into_response();
        }
    }
    match spawn_session(app).await {
        Ok((id, port, password, state)) => {
            let cookie = format!("{COOKIE}={id}; Path=/; HttpOnly; SameSite=Lax; Max-Age=86400");
            let url = format!("http://{}:{port}/", host(app, Some(headers)));
            let mut response = Html(page(
                "Anthracite demo",
                &format!(
                    "<p>This browser owns a private demo session: its files, users, \
                     routes and mounts persist for this session and are never shared \
                     with another one.</p>\
                     <p><a href=\"{url}\">Open the panel</a></p>\
                     <p>Operator <code>operator</code>, password \
                     <code>{}</code>.</p>\
                     <p>The state lives in <code>{}</code>.</p>\
                     <p><a href=\"/new\">Start a new session</a>; this one keeps its \
                     state, but the browser moves to the new one.</p>",
                    escape(&password),
                    escape(&state.display().to_string())
                ),
            ))
            .into_response();
            response
                .headers_mut()
                .insert(header::SET_COOKIE, cookie.parse().expect("a valid cookie"));
            response
        }
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Html(page(
                "The demo could not start",
                &format!(
                    "<p>{}</p><p>Read the panel log under <code>{}</code>.</p>",
                    escape(&error),
                    escape(&app.config.state.display().to_string())
                ),
            )),
        )
            .into_response(),
    }
}

async fn spawn_session(app: &Arc<App>) -> Result<(String, u16, String, PathBuf), String> {
    let sequence = app.next.fetch_add(1, Ordering::SeqCst);
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let id = format!("{stamp:x}-{sequence:x}");
    let state = app.config.state.join("sessions").join(&id);
    std::fs::create_dir_all(&state).map_err(|error| format!("creating state: {error}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700));
    }
    let port = free_port().map_err(|error| format!("choosing a port: {error}"))?;
    let socket = state.join("engine.sock");
    let config_path = state.join("config.toml");
    let config = format!(
        "version = 1\nsocket = \"{}\"\nstate = \"{}\"\n\n[apps]\nrequire-signature = true\n\
         allow-unverified-assets = false\n\n[services]\nallow-third-party = false\n\n\
         [limits]\nstart-timeout-ms = 5000\n",
        socket.display(),
        state.display()
    );
    std::fs::write(&config_path, config).map_err(|error| format!("writing config: {error}"))?;
    let log = std::fs::File::create(state.join("panel.log"))
        .map_err(|error| format!("opening the panel log: {error}"))?;

    let loopback = app.config.listen.ip().is_loopback();
    let mut command = Command::new(&app.config.panel);
    command
        .arg("--listen")
        .arg(format!("{}:{port}", app.config.listen.ip()))
        .arg("--config")
        .arg(&config_path);
    if !loopback {
        command.arg("--allow-remote");
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::from(
            log.try_clone().map_err(|error| error.to_string())?,
        ))
        .stderr(Stdio::from(log))
        .spawn()
        .map_err(|error| format!("starting the panel: {error}"))?;

    let password = match initialize(app, &socket, &id).await {
        Ok(password) => password,
        Err(error) => {
            let _ = child.kill().await;
            return Err(error);
        }
    };

    let mut sessions = app.sessions.lock().await;
    sessions.insert(
        id.clone(),
        Session {
            port,
            state: state.clone(),
            password: password.clone(),
            child,
        },
    );
    println!("session {id} on port {port} (state {})", state.display());
    Ok((id, port, password, state))
}

async fn initialize(app: &Arc<App>, socket: &Path, id: &str) -> Result<String, String> {
    wait_for(app, socket, 300).await?;
    // The generated first-boot password is useless to a visitor who cannot read
    // the server's state; the launcher sets a session password and shows it on
    // the landing page. The switch clears the first-boot gate.
    let password = format!("demo-{id}");
    cli_with_input(app, socket, &["operator", "password"], &password).await?;
    for app_name in APPS {
        let bundle = app.config.bundles.join(format!("{app_name}.anth"));
        let id = format!("org.anthracite.{app_name}");
        cli(
            app,
            socket,
            &["app", "install", &bundle.display().to_string()],
        )
        .await?;
        let mut args = vec!["app", "approve", &id, "--accept-write", "--accept-admin"];
        if HOST_APPS.contains(app_name) {
            args.push("--accept-host");
        }
        cli(app, socket, &args).await?;
    }
    let ids: Vec<String> = APPS
        .iter()
        .map(|app_name| format!("org.anthracite.{app_name}"))
        .collect();
    let mut compose: Vec<&str> = vec!["compose"];
    for id in &ids {
        compose.push("--add");
        compose.push(id);
    }
    compose.push("--now");
    cli(app, socket, &compose).await?;
    wait_for(app, socket, 300).await?;
    Ok(password)
}

async fn wait_for(app: &Arc<App>, socket: &Path, tries: u32) -> Result<(), String> {
    for _ in 0..tries {
        let output = Command::new(&app.config.cli)
            .arg("--socket")
            .arg(socket)
            .arg("status")
            .output()
            .await;
        if matches!(output, Ok(output) if output.status.success()) {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    Err(format!(
        "the panel did not become ready at {}",
        socket.display()
    ))
}

async fn cli(app: &Arc<App>, socket: &Path, args: &[&str]) -> Result<(), String> {
    let output = Command::new(&app.config.cli)
        .arg("--socket")
        .arg(socket)
        .args(args)
        .output()
        .await
        .map_err(|error| format!("running the CLI: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(format!(
        "`anth {}` failed: {}",
        args.join(" "),
        stderr.trim()
    ))
}

async fn cli_with_input(
    app: &Arc<App>,
    socket: &Path,
    args: &[&str],
    input: &str,
) -> Result<(), String> {
    let mut child = Command::new(&app.config.cli)
        .arg("--socket")
        .arg(socket)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("running the CLI: {error}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(input.as_bytes())
            .await
            .map_err(|error| format!("writing the password: {error}"))?;
        stdin
            .write_all(b"\n")
            .await
            .map_err(|error| format!("writing the password: {error}"))?;
    }
    let output = child
        .wait_with_output()
        .await
        .map_err(|error| format!("waiting for the CLI: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "`anth {}` failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr).trim()
    ))
}

fn free_port() -> std::io::Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

async fn shutdown(app: &Arc<App>) -> Result<(), String> {
    let mut sessions = app.sessions.lock().await;
    for (_, mut session) in std::mem::take(&mut *sessions) {
        let _ = session.child.kill().await;
    }
    Ok(())
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn page(title: &str, body: &str) -> String {
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <title>{title} · Anthracite demo</title>\
         <style>body{{font:15px system-ui;background:#0b0d12;color:#e8ebf2;margin:0;\
         display:grid;place-items:center;min-height:100vh}}main{{max-width:34rem;\
         padding:2rem}}a{{color:#9d7bff}}code{{color:#b9c3d6}}</style></head>\
         <body><main><h1>{title}</h1>{body}</main></body></html>",
        title = escape(title),
        body = body
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cookie_is_read_from_the_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            format!("other=1; {COOKIE}=abc-1; tail=2").parse().unwrap(),
        );
        assert_eq!(cookie_session(&headers), Some("abc-1".to_string()));
        assert_eq!(cookie_session(&HeaderMap::new()), None);
    }

    #[test]
    fn an_empty_or_oversized_cookie_is_ignored() {
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, format!("{COOKIE}=").parse().unwrap());
        assert_eq!(cookie_session(&headers), None);
        headers.insert(
            header::COOKIE,
            format!("{COOKIE}={}", "a".repeat(97)).parse().unwrap(),
        );
        assert_eq!(cookie_session(&headers), None);
    }
}
