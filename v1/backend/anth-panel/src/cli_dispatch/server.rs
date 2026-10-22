pub const DEFAULT_ADDR: &str = "0.0.0.0:8080";

use super::super::main_prelude::*;

pub fn resolve_addr(args: &EndpointArgs) -> String {
    args.addr
        .clone()
        .or_else(|| std::env::var("ANTH_PANEL_ADDR").ok())
        .unwrap_or_else(|| DEFAULT_ADDR.to_string())
}

pub async fn serve_api(
    config: Config,
    config_path: &str,
    args: &EndpointArgs,
    ready: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
) -> Result<ExitCode, String> {
    let address = resolve_addr(args);
    let listener = match TcpListener::bind(&address).await {
        Ok(listener) => listener,
        Err(error) => {
            let detail = format!("Failed to bind to address {address}: {error}");
            notify_ready(ready, Err(detail.clone()));
            return Err(detail);
        }
    };

    println!("Server is listening on {address}");
    notify_ready(ready, Ok(()));
    let state = AppState::with_config_path(config, Path::new(config_path));
    axum::serve(
        listener,
        router(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .map_err(|e| format!("Server error: {e}"))?;
    Ok(ExitCode::SUCCESS)
}

/// Hands the bind outcome to a waiting [`dispatch`] without making the serving
/// functions depend on it. A dropped receiver is not an error: a one-shot
/// invocation passes `None`, and the interactive session is the only caller
/// that waits.
pub fn notify_ready(
    ready: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
    outcome: Result<(), String>,
) {
    if let Some(ready) = ready {
        let _ = ready.send(outcome);
    }
}

/// Keeps the daemon task alive for as long as the API runs: binding the HTTP
/// port first surfaces address-in-use errors before the socket is touched.
pub async fn serve_run(
    config: Config,
    config_path: &str,
    args: &EndpointArgs,
    ready: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
) -> Result<ExitCode, String> {
    let address = resolve_addr(args);
    let listener = match TcpListener::bind(&address).await {
        Ok(listener) => listener,
        Err(error) => {
            let detail = format!("Failed to bind to address {address}: {error}");
            notify_ready(ready, Err(detail.clone()));
            return Err(detail);
        }
    };

    let daemon_config = config.clone();
    let daemon_config_path = std::path::PathBuf::from(config_path);
    let (daemon_ready, daemon_bound) = tokio::sync::oneshot::channel();
    let daemon = tokio::spawn(async move {
        if let Err(e) = anth_panel::daemon::serve_with_ready(
            &daemon_config,
            &daemon_config_path,
            Some(daemon_ready),
        )
        .await
        {
            eprintln!("Daemon stopped: {e}");
        }
    });

    // The HTTP port is bound, but `run` is not ready until the socket is too:
    // an interactive session sends its first request as soon as this returns.
    match daemon_bound.await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            daemon.abort();
            let _ = daemon.await;
            notify_ready(ready, Err(error.clone()));
            return Err(error);
        }
        Err(_) => {
            daemon.abort();
            let _ = daemon.await;
            let detail = "the daemon stopped before binding the socket".to_string();
            notify_ready(ready, Err(detail.clone()));
            return Err(detail);
        }
    }

    println!("Daemon and server are running (http://{address})");
    notify_ready(ready, Ok(()));
    let state = AppState::with_config_path(config, Path::new(config_path));
    let result = axum::serve(
        listener,
        router(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .map_err(|e| format!("Server error: {e}"));

    daemon.abort();
    let _ = daemon.await;
    result.map(|_| ExitCode::SUCCESS)
}

/// Which services one detached session task serves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerRole {
    Daemon,
    Api,
    Run,
}

impl ServerRole {
    pub fn of(command: &Command) -> Option<Self> {
        match command {
            Command::Daemon => Some(Self::Daemon),
            Command::Api(_) => Some(Self::Api),
            Command::Run(_) => Some(Self::Run),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Daemon => "daemon",
            Self::Api => "api",
            Self::Run => "run",
        }
    }

    /// Whether a running task in `self` already binds what `requested` needs.
    ///
    /// `run` owns both the socket and the HTTP port, so it conflicts with a
    /// separate `daemon` or `api` in either direction. `daemon` and `api` do not
    /// conflict with each other: starting both is the point of the session.
    pub fn serves(self, requested: Self) -> bool {
        self == requested || self == Self::Run || requested == Self::Run
    }
}

pub struct BackgroundTask {
    role: ServerRole,
    handle: tokio::task::JoinHandle<()>,
}

/// Server subcommands the interactive session left running behind the prompt.
///
/// The prompt keeps reading lines while these serve, so one terminal can run the
/// daemon and the API and still send requests. A duplicate start is refused
/// rather than detached: `daemon::serve` unlinks the socket before binding, so a
/// second copy would steal the path from a live one and leave it unreachable.
#[derive(Default)]
pub struct BackgroundServers {
    tasks: Vec<BackgroundTask>,
}

impl BackgroundServers {
    /// Refuses a start that would collide with a server already detached.
    ///
    /// Kept separate from the spawning so no `RefCell` borrow is held across the
    /// readiness await: this call only inspects the registry.
    pub fn check(&mut self, command: &Command) -> Result<ServerRole, String> {
        let role = ServerRole::of(command).ok_or("not a server subcommand")?;

        // A task that already finished (a busy port, an unbindable socket) must
        // not block a retry.
        self.tasks.retain(|task| !task.handle.is_finished());
        if let Some(running) = self.tasks.iter().find(|task| task.role.serves(role)) {
            return Err(if running.role == role {
                format!(
                    "{} is already running in the background; quit the session to stop it",
                    role.label()
                )
            } else {
                format!(
                    "cannot start `{}`: `{}` already owns what it would bind; quit the session to stop it",
                    role.label(),
                    running.role.label()
                )
            });
        }
        Ok(role)
    }

    pub fn register(&mut self, role: ServerRole, handle: tokio::task::JoinHandle<()>) {
        self.tasks.push(BackgroundTask { role, handle });
    }

    /// Stops every detached server once the prompt closes.
    ///
    /// Aborting is enough: the session returns to `main` immediately, and
    /// dropping the runtime finishes the cancellation.
    pub fn shutdown(&mut self) {
        for task in self.tasks.drain(..) {
            task.handle.abort();
        }
    }
}

/// Spawns one serving subcommand and waits until it has bound its port.
///
/// The interactive session detaches a `daemon`, `api` or `run` and keeps its
/// prompt, so the next typed line is already waiting: without this handshake a
/// request could reach the socket before the listener exists.
/// Loads the config for a server subcommand, turning a refusal into the `String`
/// error the dispatch already returns.
///
/// A config that exists but will not parse stops the server instead of being
/// replaced: the alternative silently discards the operator's settings, which is
/// how a stale plugin grant once turned into a wiped `files.root`.
pub fn loaded_config(config_path: &str) -> Config {
    match Config::load_or_create(config_path) {
        Ok(config) => config,
        Err(error) => {
            // A parse error is fatal for a server: continuing with defaults
            // would run the panel with settings nobody chose.
            eprintln!("[ERROR] {error}");
            std::process::exit(1);
        }
    }
}

pub async fn start_server(
    config_path: &str,
    command: &Command,
) -> Result<tokio::task::JoinHandle<()>, String> {
    let config = match Config::load_or_create(config_path) {
        Ok(config) => config,
        Err(error) => return Err(error),
    };
    let path = config_path.to_string();
    let (ready, bound) = tokio::sync::oneshot::channel();
    let handle = match command {
        Command::Daemon => tokio::spawn(async move {
            if let Err(error) =
                anth_panel::daemon::serve_with_ready(&config, Path::new(&path), Some(ready)).await
            {
                eprintln!("[ERROR] daemon stopped: {error}");
            }
        }),
        Command::Api(args) => {
            let args = args.clone();
            tokio::spawn(async move {
                if let Err(error) = serve_api(config, &path, &args, Some(ready)).await {
                    eprintln!("[ERROR] api stopped: {error}");
                }
            })
        }
        Command::Run(args) => {
            let args = args.clone();
            tokio::spawn(async move {
                if let Err(error) = serve_run(config, &path, &args, Some(ready)).await {
                    eprintln!("[ERROR] run stopped: {error}");
                }
            })
        }
        _ => unreachable!("only a server subcommand is detached"),
    };

    match bound.await {
        Ok(Ok(())) => Ok(handle),
        Ok(Err(_)) => {
            let _ = handle.await;
            Err("the service could not start; see the error above".to_string())
        }
        Err(_) => {
            let _ = handle.await;
            Err("the service stopped before it was listening".to_string())
        }
    }
}
