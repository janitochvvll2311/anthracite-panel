//! `anth-demo-sessions`: one panel and one emulator state per browser session.
//!
//! The public demo runs on its own origin behind an operator's proxy. Each
//! visitor solves a Cloudflare Turnstile challenge, gets a short-lived session
//! with its own panel and emulator state, and is signed in without an operator
//! password. Sessions expire and their state is deleted. Creation is limited
//! per address and globally, repeated failures block an address, and a
//! cross-origin form cannot start a session.
//!
//! This is demo tooling, not a deployment mode: loopback by default, a session
//! cap and no proxy of its own.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::net::{IpAddr, SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::extract::{ConnectInfo, Form, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::Router;
use reqwest::Client;
use serde_json::json;
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

const COOKIE: &str = "anth_demo_session";
const ENGINE_COOKIE: &str = "anth_session";
const APPS: &[&str] = &[
    "accounts", "firewall", "fs", "home", "net", "proc", "storage", "terminal",
];
const HOST_APPS: &[&str] = &["home"];

const TURNSTILE_SCRIPT: &str = "https://challenges.cloudflare.com/turnstile/v0/api.js";
const TURNSTILE_VERIFY: &str = "https://challenges.cloudflare.com/turnstile/v0/siteverify";
const TURNSTILE_FIELD: &str = "cf-turnstile-response";
const TURNSTILE_CLASS: &str = "cf-turnstile";
const TURNSTILE_ORIGIN: &str = "https://challenges.cloudflare.com";
const TURNSTILE_ACTION: &str = "demo-session";

/// One address may create three demos an hour; the whole launcher six a minute.
const CREATIONS_PER_IP_HOUR: usize = 3;
const CREATIONS_PER_MINUTE: usize = 6;
const CREATIONS_PER_HOUR: usize = 60;
/// A browser gets ten attempts a minute; five failures block it for an hour.
const ATTEMPTS_PER_MINUTE: usize = 10;
const FAILURES_MAX: usize = 5;
const FAILURES_WINDOW: Duration = Duration::from_secs(10 * 60);
const BLOCK: Duration = Duration::from_secs(60 * 60);
/// Bound the per-address maps so a spray of source addresses cannot grow them.
const MAX_ADDRESSES: usize = 4096;

#[derive(Clone)]
struct Turnstile {
    site_key: String,
    secret: String,
    hostname: Option<String>,
    verify_url: String,
}

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
    ttl: Duration,
    trust_proxy: bool,
    captcha: Option<Turnstile>,
    /// The first port of the child range; zero means an ephemeral port.
    port_base: u16,
    port_count: u16,
}

struct Session {
    port: u16,
    state: PathBuf,
    password: String,
    created: Instant,
    child: Child,
}

#[derive(Default)]
struct Limiter {
    attempts: HashMap<String, VecDeque<Instant>>,
    creations: HashMap<String, VecDeque<Instant>>,
    failures: HashMap<String, VecDeque<Instant>>,
    blocked: HashMap<String, Instant>,
    global_minute: VecDeque<Instant>,
    global_hour: VecDeque<Instant>,
}

struct App {
    config: Config,
    sessions: Mutex<BTreeMap<String, Session>>,
    create_lock: Mutex<()>,
    limiter: Mutex<Limiter>,
    next: AtomicU64,
    client: Client,
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
             through the operator's proxy.",
            config.listen
        );
    }
    if let Err(error) = std::fs::create_dir_all(&config.state) {
        eprintln!("creating {}: {error}", config.state.display());
        return std::process::ExitCode::from(1);
    }
    let captcha = if config.captcha.is_some() {
        "turnstile"
    } else {
        "none"
    };
    let app = Arc::new(App {
        config: config.clone(),
        sessions: Mutex::new(BTreeMap::new()),
        create_lock: Mutex::new(()),
        limiter: Mutex::new(Limiter::default()),
        next: AtomicU64::new(1),
        client: Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| Client::new()),
    });

    let router = Router::new()
        .route("/", get(root))
        .route("/start", post(start))
        .route("/new", get(landing))
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
        "anth-demo-sessions listening on http://{} (state {}, turnstile {}, ttl {}s)",
        config.listen,
        config.state.display(),
        captcha,
        config.ttl.as_secs()
    );

    if !config.ttl.is_zero() {
        let sweeper = Arc::clone(&app);
        let ttl = config.ttl;
        tokio::spawn(async move {
            let every = std::cmp::max(
                Duration::from_secs(1),
                std::cmp::min(ttl / 2, Duration::from_secs(30)),
            );
            let mut interval = tokio::time::interval(every);
            loop {
                interval.tick().await;
                sweep(&sweeper).await;
            }
        });
    }

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

    let served = axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await;
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
    let mut ttl_seconds = 1800u64;
    let mut port_base = 0u16;
    let mut port_count = 100u16;
    let mut trust_proxy = env_flag("ANTHRACITE_TRUST_PROXY");
    let mut site_key = std::env::var("ANTHRACITE_CAPTCHA_SITE_KEY").ok();
    let mut hostname = std::env::var("ANTHRACITE_CAPTCHA_HOSTNAME").ok();
    let mut verify_url = None;
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
            "--ttl-seconds" => {
                let value = args.next().ok_or("`--ttl-seconds` needs a number")?;
                ttl_seconds = value
                    .parse()
                    .map_err(|_| "`--ttl-seconds` needs a number".to_string())?;
            }
            "--port-base" => {
                let value = args.next().ok_or("`--port-base` needs a port")?;
                port_base = value
                    .parse()
                    .map_err(|_| "`--port-base` needs a port".to_string())?;
            }
            "--port-count" => {
                let value = args.next().ok_or("`--port-count` needs a number")?;
                port_count = value
                    .parse()
                    .map_err(|_| "`--port-count` needs a positive number".to_string())?;
            }
            "--trust-proxy" => trust_proxy = true,
            "--captcha-site-key" => site_key = args.next(),
            "--captcha-hostname" => hostname = args.next(),
            "--captcha-verify-url" => verify_url = args.next(),
            "--help" | "-h" => {
                println!(
                    "usage: anth-demo-sessions --panel <path> --cli <path> --bundles <dir> \
                     [--state <dir>] [--listen <addr>] [--public-host <host>] [--max <n>] \
                     [--ttl-seconds <n>] [--port-base <n>] [--port-count <n>] [--trust-proxy] \
                     [--captcha-site-key <key>] [--captcha-hostname <host>] \
                     [--captcha-verify-url <url>]\n\
                     Turnstile is configured with ANTHRACITE_CAPTCHA_SITE_KEY (or the flag), \
                     ANTHRACITE_CAPTCHA_SECRET (env only) and ANTHRACITE_CAPTCHA_HOSTNAME."
                );
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument `{other}`")),
        }
    }
    let secret = std::env::var("ANTHRACITE_CAPTCHA_SECRET")
        .ok()
        .filter(|value| !value.is_empty());
    let captcha = match (site_key, secret) {
        (None, None) => None,
        (Some(site_key), Some(secret)) if !site_key.is_empty() => Some(Turnstile {
            site_key,
            secret,
            hostname: hostname.filter(|value| !value.is_empty()),
            verify_url: verify_url.unwrap_or_else(|| TURNSTILE_VERIFY.to_string()),
        }),
        _ => {
            return Err("turnstile needs both a site key and ANTHRACITE_CAPTCHA_SECRET".to_string())
        }
    };
    if captcha.is_none() && (public_host.is_some() || !listen.ip().is_loopback()) {
        return Err(
            "a public demo needs Turnstile: set ANTHRACITE_CAPTCHA_SITE_KEY and \
             ANTHRACITE_CAPTCHA_SECRET"
                .to_string(),
        );
    }
    if (public_host.is_some() || !listen.ip().is_loopback())
        && captcha.as_ref().and_then(|c| c.hostname.as_ref()).is_none()
    {
        return Err(
            "a public demo needs ANTHRACITE_CAPTCHA_HOSTNAME so a token from another \
             site cannot be replayed"
                .to_string(),
        );
    }
    Ok(Config {
        panel: panel.ok_or("`--panel` is required")?,
        cli: cli.ok_or("`--cli` is required")?,
        bundles: bundles.ok_or("`--bundles` is required")?,
        state,
        listen,
        public_host,
        max,
        ttl: Duration::from_secs(ttl_seconds),
        trust_proxy,
        captcha,
        port_base,
        port_count,
    })
}

fn env_flag(name: &str) -> bool {
    std::env::var(name)
        .map(|value| matches!(value.as_str(), "1" | "true" | "yes"))
        .unwrap_or(false)
}

async fn landing(State(app): State<Arc<App>>) -> Response {
    landing_page(&app)
}

async fn root(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    match cookie_session(&headers) {
        Some(id) => match session_redirect(&app, &id, &headers).await {
            Some(response) => response,
            None => landing_page(&app),
        },
        None => landing_page(&app),
    }
}

async fn session_redirect(app: &App, id: &str, headers: &HeaderMap) -> Option<Response> {
    let sessions = app.sessions.lock().await;
    let session = sessions.get(id)?;
    if session.created.elapsed() >= app.config.ttl {
        return None;
    }
    Some(
        Redirect::to(&format!(
            "{}://{}:{}/",
            scheme(headers),
            host(app, Some(headers)),
            session.port
        ))
        .into_response(),
    )
}

async fn start(
    State(app): State<Arc<App>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    let ip = client_ip(&headers, peer, app.config.trust_proxy);
    let expected = host(&app, Some(&headers));
    if !origin_ok(&headers, &expected) {
        println!("origin refused for {ip}");
        return error_page(
            StatusCode::FORBIDDEN,
            "A cross-origin form cannot start a demo session.",
        );
    }
    if let Some(wait) = app.limiter.lock().await.blocked_for(&ip, Instant::now()) {
        return error_page(
            StatusCode::TOO_MANY_REQUESTS,
            &format!(
                "This address is temporarily blocked; try again in {} minutes.",
                wait.as_secs() / 60 + 1
            ),
        );
    }
    if let Err(reason) = app.limiter.lock().await.record_attempt(&ip, Instant::now()) {
        println!("rate limit {ip}: {reason}");
        return error_page(StatusCode::TOO_MANY_REQUESTS, &reason);
    }
    if let Some(captcha) = &app.config.captcha {
        let token = form
            .get(TURNSTILE_FIELD)
            .map(String::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if token.is_empty() {
            return captcha_failure(&app, &ip, "no token").await;
        }
        if let Err(reason) = verify_turnstile(&app, captcha, &token, &ip).await {
            return captcha_failure(&app, &ip, &reason).await;
        }
    }
    app.limiter.lock().await.record_success(&ip);
    if let Err(reason) = app
        .limiter
        .lock()
        .await
        .permit_creation(&ip, Instant::now())
    {
        println!("rate limit {ip}: {reason}");
        return error_page(StatusCode::TOO_MANY_REQUESTS, &reason);
    }
    match create_session(&app, &headers, &ip).await {
        Ok(response) => response,
        Err(error) => error_page(StatusCode::INTERNAL_SERVER_ERROR, &error),
    }
}

async fn captcha_failure(app: &Arc<App>, ip: &str, reason: &str) -> Response {
    let blocked = app.limiter.lock().await.record_failure(ip, Instant::now());
    match blocked {
        Some(_) => {
            println!("captcha failure for {ip}: {reason}; blocked for an hour");
            error_page(
                StatusCode::TOO_MANY_REQUESTS,
                "Too many failed challenges. This address is blocked for an hour.",
            )
        }
        None => {
            println!("captcha failure for {ip}: {reason}");
            error_page(
                StatusCode::BAD_REQUEST,
                "The challenge was not accepted. Try again.",
            )
        }
    }
}

async fn verify_turnstile(
    app: &Arc<App>,
    captcha: &Turnstile,
    token: &str,
    ip: &str,
) -> Result<(), String> {
    let response = app
        .client
        .post(&captcha.verify_url)
        .form(&[
            ("secret", captcha.secret.as_str()),
            ("response", token),
            ("remoteip", ip),
        ])
        .send()
        .await
        .map_err(|error| format!("unreachable: {error}"))?;
    let value: serde_json::Value = response
        .json()
        .await
        .map_err(|error| format!("invalid answer: {error}"))?;
    if value["success"].as_bool() != Some(true) {
        return Err("refused".to_string());
    }
    if value["action"].as_str() != Some(TURNSTILE_ACTION) {
        return Err("wrong action".to_string());
    }
    if let Some(hostname) = &captcha.hostname {
        if value["hostname"].as_str() != Some(hostname.as_str()) {
            return Err("wrong hostname".to_string());
        }
    }
    Ok(())
}

impl Limiter {
    fn bound(&mut self) {
        if self.attempts.len() > MAX_ADDRESSES {
            self.attempts.clear();
        }
        if self.creations.len() > MAX_ADDRESSES {
            self.creations.clear();
        }
        if self.failures.len() > MAX_ADDRESSES {
            self.failures.clear();
        }
    }

    fn blocked_for(&mut self, ip: &str, now: Instant) -> Option<Duration> {
        match self.blocked.get(ip) {
            Some(until) if *until > now => Some(*until - now),
            Some(_) => {
                self.blocked.remove(ip);
                None
            }
            None => None,
        }
    }

    fn record_attempt(&mut self, ip: &str, now: Instant) -> Result<(), String> {
        self.bound();
        let attempts = self.attempts.entry(ip.to_string()).or_default();
        prune(attempts, Duration::from_secs(60), now);
        if attempts.len() >= ATTEMPTS_PER_MINUTE {
            return Err("Too many attempts; wait a minute.".to_string());
        }
        attempts.push_back(now);
        Ok(())
    }

    fn permit_creation(&mut self, ip: &str, now: Instant) -> Result<(), String> {
        self.bound();
        let creations = self.creations.entry(ip.to_string()).or_default();
        prune(creations, Duration::from_secs(3600), now);
        if creations.len() >= CREATIONS_PER_IP_HOUR {
            return Err("This address has used its demo sessions for now.".to_string());
        }
        prune(&mut self.global_minute, Duration::from_secs(60), now);
        if self.global_minute.len() >= CREATIONS_PER_MINUTE {
            return Err("The demo is busy; try again in a minute.".to_string());
        }
        prune(&mut self.global_hour, Duration::from_secs(3600), now);
        if self.global_hour.len() >= CREATIONS_PER_HOUR {
            return Err("The demo is busy right now; try again later.".to_string());
        }
        creations.push_back(now);
        self.global_minute.push_back(now);
        self.global_hour.push_back(now);
        Ok(())
    }

    fn record_failure(&mut self, ip: &str, now: Instant) -> Option<Duration> {
        self.bound();
        let failures = self.failures.entry(ip.to_string()).or_default();
        prune(failures, FAILURES_WINDOW, now);
        failures.push_back(now);
        if failures.len() >= FAILURES_MAX {
            self.blocked.insert(ip.to_string(), now + BLOCK);
            failures.clear();
            return Some(BLOCK);
        }
        None
    }

    fn record_success(&mut self, ip: &str) {
        self.failures.remove(ip);
    }
}

fn prune(queue: &mut VecDeque<Instant>, window: Duration, now: Instant) {
    while let Some(front) = queue.front() {
        if now.duration_since(*front) > window {
            queue.pop_front();
        } else {
            break;
        }
    }
}

async fn create_session(app: &Arc<App>, headers: &HeaderMap, ip: &str) -> Result<Response, String> {
    let _guard = app.create_lock.lock().await;
    {
        let sessions = app.sessions.lock().await;
        if sessions.len() >= app.config.max {
            return Err("Every demo session is in use; try again in a few minutes.".to_string());
        }
    }
    let (id, session) = spawn_session(app).await?;
    let secure = is_https(headers);
    let engine_cookie = match login(app, session.port, &session.password).await {
        Ok(cookie) => cookie,
        Err(error) => {
            let mut failed = session;
            let _ = failed.child.kill().await;
            let _ = std::fs::remove_dir_all(&failed.state);
            return Err(error);
        }
    };
    let engine_cookie = if secure && !engine_cookie.to_ascii_lowercase().contains("secure") {
        format!("{engine_cookie}; Secure")
    } else {
        engine_cookie
    };
    let demo_cookie = format!(
        "{COOKIE}={id}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}{}",
        app.config.ttl.as_secs(),
        if secure { "; Secure" } else { "" }
    );
    let target = format!(
        "{}://{}:{}/",
        scheme(headers),
        host(app, Some(headers)),
        session.port
    );
    let mut response = Redirect::to(&target).into_response();
    for cookie in [demo_cookie, engine_cookie] {
        response.headers_mut().append(
            header::SET_COOKIE,
            HeaderValue::from_str(&cookie).map_err(|error| format!("setting a cookie: {error}"))?,
        );
    }
    println!(
        "session {id} created for {ip} on port {} (state {})",
        session.port,
        session.state.display()
    );
    {
        let mut sessions = app.sessions.lock().await;
        sessions.insert(id, session);
    }
    Ok(response)
}

async fn spawn_session(app: &Arc<App>) -> Result<(String, Session), String> {
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
    let port = allocate_port(app).map_err(|error| format!("choosing a port: {error}"))?;
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

    let password = random_password();
    if let Err(error) = initialize(app, &socket, &password).await {
        let _ = child.kill().await;
        return Err(error);
    }
    let session = Session {
        port,
        state,
        password,
        created: Instant::now(),
        child,
    };
    Ok((id, session))
}

async fn initialize(app: &Arc<App>, socket: &Path, password: &str) -> Result<(), String> {
    wait_for(app, socket, 300).await?;
    // The generated first-boot password is replaced with a random one; the
    // visitor never sees it because the launcher logs in on their behalf.
    cli_with_input(app, socket, &["operator", "password"], password).await?;
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
    wait_for(app, socket, 300).await
}

async fn login(app: &Arc<App>, port: u16, password: &str) -> Result<String, String> {
    let response = app
        .client
        .post(format!("http://127.0.0.1:{port}/api/engine/session"))
        .json(&json!({"operator": "operator", "password": password}))
        .send()
        .await
        .map_err(|error| format!("signing the visitor in: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "signing the visitor in: the panel answered {}",
            response.status()
        ));
    }
    let cookies = response.headers().get_all(header::SET_COOKIE);
    for value in cookies {
        if let Ok(text) = value.to_str() {
            if text.starts_with(&format!("{ENGINE_COOKIE}=")) {
                return Ok(text.to_string());
            }
        }
    }
    Err("signing the visitor in: the panel set no session cookie".to_string())
}

async fn sweep(app: &Arc<App>) {
    if app.config.ttl.is_zero() {
        return;
    }
    let expired: Vec<(String, Session)> = {
        let mut sessions = app.sessions.lock().await;
        let ids: Vec<String> = sessions
            .iter()
            .filter(|(_, session)| session.created.elapsed() >= app.config.ttl)
            .map(|(id, _)| id.clone())
            .collect();
        ids.into_iter()
            .filter_map(|id| sessions.remove(&id).map(|session| (id, session)))
            .collect()
    };
    for (id, mut session) in expired {
        let _ = session.child.kill().await;
        let _ = std::fs::remove_dir_all(&session.state);
        println!("session {id} expired and its state was removed");
    }
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
        tokio::time::sleep(Duration::from_millis(100)).await;
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
    Err(format!(
        "`anth {}` failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr).trim()
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

fn random_password() -> String {
    let mut bytes = [0u8; 16];
    if getrandom::fill(&mut bytes).is_err() {
        // A demo must still start if the OS has no randomness; this is not a
        // security boundary for the visitor, who is signed in by the launcher.
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        bytes = now.to_le_bytes()[..16].try_into().unwrap_or([7u8; 16]);
    }
    let text: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("demo-{text}")
}

fn free_port() -> std::io::Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

/// A child port: ephemeral by default, or the first free one in the configured
/// range so a container image can publish it.
fn allocate_port(app: &App) -> Result<u16, String> {
    if app.config.port_base == 0 {
        return free_port().map_err(|error| error.to_string());
    }
    let ip = app.config.listen.ip();
    let count = u32::from(app.config.port_count.max(1));
    let start = u32::from(app.config.port_base);
    for port in start..start + count {
        if port > u32::from(u16::MAX) {
            break;
        }
        if TcpListener::bind((ip, port as u16)).is_ok() {
            return Ok(port as u16);
        }
    }
    Err(format!(
        "no free session port in {start}..{}",
        start + count
    ))
}

async fn shutdown(app: &Arc<App>) -> Result<(), String> {
    let mut sessions = app.sessions.lock().await;
    for (_, mut session) in std::mem::take(&mut *sessions) {
        let _ = session.child.kill().await;
    }
    Ok(())
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

/// The address a limit applies to. Only a trusted proxy may claim one; a direct
/// client cannot pick its own bucket with a header.
fn client_ip(headers: &HeaderMap, peer: SocketAddr, trust_proxy: bool) -> String {
    if trust_proxy {
        for name in ["cf-connecting-ip", "x-real-ip"] {
            if let Some(value) = headers.get(name).and_then(|value| value.to_str().ok()) {
                if let Ok(ip) = value.trim().parse::<IpAddr>() {
                    return ip.to_string();
                }
            }
        }
        if let Some(value) = headers
            .get("x-forwarded-for")
            .and_then(|value| value.to_str().ok())
        {
            if let Some(first) = value.split(',').next() {
                if let Ok(ip) = first.trim().parse::<IpAddr>() {
                    return ip.to_string();
                }
            }
        }
    }
    peer.ip().to_string()
}

fn origin_ok(headers: &HeaderMap, expected: &str) -> bool {
    for name in ["origin", "referer"] {
        if let Some(value) = headers.get(name).and_then(|value| value.to_str().ok()) {
            let Some(rest) = value.split_once("://").map(|(_, rest)| rest) else {
                return false;
            };
            let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
            let host = authority
                .rsplit_once(':')
                .map(|(host, _)| host)
                .unwrap_or(authority);
            if !host.eq_ignore_ascii_case(expected) {
                return false;
            }
        }
    }
    true
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

/// The public scheme, as the operator's proxy reports it.
fn is_https(headers: &HeaderMap) -> bool {
    headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.eq_ignore_ascii_case("https"))
        .unwrap_or(false)
}

fn scheme(headers: &HeaderMap) -> &'static str {
    if is_https(headers) {
        "https"
    } else {
        "http"
    }
}

fn landing_page(app: &App) -> Response {
    let (script, widget, csp) = match &app.config.captcha {
        Some(captcha) => (
            format!("<script src=\"{TURNSTILE_SCRIPT}\" async defer></script>"),
            format!(
                "<div class=\"{TURNSTILE_CLASS}\" data-sitekey=\"{}\" \
                 data-action=\"{TURNSTILE_ACTION}\"></div>",
                escape(&captcha.site_key)
            ),
            format!(
                "default-src 'self'; style-src 'self' 'unsafe-inline'; \
                 script-src 'self' {TURNSTILE_ORIGIN}; frame-src {TURNSTILE_ORIGIN}; \
                 connect-src 'self' {TURNSTILE_ORIGIN}; base-uri 'none'; form-action 'self'"
            ),
        ),
        None => (
            String::new(),
            String::new(),
            "default-src 'self'; style-src 'self' 'unsafe-inline'; \
             base-uri 'none'; form-action 'self'"
                .to_string(),
        ),
    };
    let ttl = app.config.ttl.as_secs() / 60;
    let body = format!(
        "<p>This is a live demo of Anthracite. Every browser gets its own panel \
         and its own emulated host; nothing you do here is visible to another \
         visitor.</p><p>The session and its state disappear after {ttl} minutes.</p>\
         <form method=\"post\" action=\"/start\">{script}{widget}\
         <p><button type=\"submit\">Open the demo</button></p></form>"
    );
    let page = page("Anthracite demo", &body);
    let mut response = Html(page).into_response();
    response.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_str(&csp).expect("a valid CSP"),
    );
    response
}

fn error_page(status: StatusCode, message: &str) -> Response {
    let body = format!(
        "<p>{}</p><p><a href=\"/\">Try again</a></p>",
        escape(message)
    );
    (status, Html(page("Anthracite demo", &body))).into_response()
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
         padding:2rem}}a{{color:#9d7bff}}code{{color:#b9c3d6}}\
         button{{padding:9px 14px;border-radius:8px;border:1px solid #3a4560;\
         background:#222b3f;color:#e8ebf2;cursor:pointer;font:inherit}}</style>\
         </head><body><main><h1>{title}</h1>{body}</main></body></html>",
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

    #[test]
    fn a_direct_client_cannot_pick_its_address_but_a_proxy_can() {
        let peer: SocketAddr = "10.0.0.9:5000".parse().unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", "203.0.113.5, 10.0.0.1".parse().unwrap());
        assert_eq!(client_ip(&headers, peer, false), "10.0.0.9");
        assert_eq!(client_ip(&headers, peer, true), "203.0.113.5");
        headers.insert("cf-connecting-ip", "198.51.100.7".parse().unwrap());
        assert_eq!(client_ip(&headers, peer, true), "198.51.100.7");
    }

    #[test]
    fn a_cross_origin_form_is_refused() {
        let mut headers = HeaderMap::new();
        assert!(origin_ok(&headers, "demo.example.com"));
        headers.insert("origin", "https://demo.example.com".parse().unwrap());
        assert!(origin_ok(&headers, "demo.example.com"));
        headers.insert("origin", "https://evil.example".parse().unwrap());
        assert!(!origin_ok(&headers, "demo.example.com"));
        headers.remove("origin");
        headers.insert(
            "referer",
            "https://demo.example.com/landing".parse().unwrap(),
        );
        assert!(origin_ok(&headers, "demo.example.com"));
    }

    #[test]
    fn attempts_creations_and_failures_are_bounded() {
        let mut limiter = Limiter::default();
        let now = Instant::now();
        for _ in 0..ATTEMPTS_PER_MINUTE {
            limiter.record_attempt("a", now).expect("attempt");
        }
        assert!(limiter.record_attempt("a", now).is_err());
        for _ in 0..CREATIONS_PER_IP_HOUR {
            limiter.permit_creation("a", now).expect("creation");
        }
        assert!(limiter.permit_creation("a", now).is_err());
        assert!(limiter.permit_creation("b", now).is_ok());
        for _ in 0..FAILURES_MAX - 1 {
            assert!(limiter.record_failure("c", now).is_none());
        }
        assert!(limiter.record_failure("c", now).is_some());
        assert!(limiter.blocked_for("c", now).is_some());
        assert!(limiter
            .blocked_for("c", now + BLOCK + Duration::from_secs(1))
            .is_none());
    }

    #[test]
    fn a_random_password_is_long_enough() {
        let password = random_password();
        assert!(password.starts_with("demo-"));
        assert!(password.len() >= 32);
        assert_ne!(password, random_password());
    }
}
