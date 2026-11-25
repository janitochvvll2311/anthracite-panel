//! The engine daemon: read the configuration, prepare the state, serve the API.
//!
//! A headless deployment runs this binary; the official panel links the same
//! crates and mounts the same router in its own process (§6.5).

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use anth_contract::Error;
use anth_engine::config::{Config, Profile};
use anth_engine::log::Logger;
use anth_engine::supervisor::Supervisor;
use anth_runtime::{LogSettings, WasmtimeRuntime};
use tokio::net::TcpListener;
use tokio::sync::watch;

struct Args {
    config: PathBuf,
    profile: Profile,
    /// Validate and exit: what the doctor command runs.
    check: bool,
    /// Delete the recovery file so the next `load_or_create` mints a new password.
    reset_credentials: bool,
}

fn main() -> ExitCode {
    let args = match Args::parse(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };
    match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| Error::internal(format!("starting the runtime: {error}")))
        .and_then(|runtime| {
            if args.check {
                runtime.block_on(check(args.config, args.profile))
            } else {
                runtime.block_on(run(args))
            }
        }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{}: {}", error.kind().code(), error.message());
            ExitCode::from(error.exit_code())
        }
    }
}

impl Args {
    fn parse(mut args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut config = None;
        let mut debug = false;
        let mut release = false;
        let mut check = false;
        let mut reset_credentials = false;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--config" => {
                    config = Some(args.next().ok_or("`--config` needs a path")?);
                }
                "--debug" => debug = true,
                "--release" => release = true,
                "--check" => check = true,
                "--reset-credentials" => reset_credentials = true,
                "--help" | "-h" => {
                    return Err(
                        "usage: anth-engine [--config <path>] [--debug|--release] [--check] \
                         [--reset-credentials]"
                            .to_string(),
                    );
                }
                other => return Err(format!("unknown argument `{other}`")),
            }
        }
        if check && reset_credentials {
            return Err(
                "`--check` is read-only and `--reset-credentials` mutates state; choose one"
                    .to_string(),
            );
        }
        Ok(Self {
            config: PathBuf::from(config.unwrap_or_else(|| "config.toml".to_string())),
            profile: choose_profile(debug, release, cfg!(debug_assertions))?,
            check,
            reset_credentials,
        })
    }
}

/// A release binary never opens the permissive profile, even when asked: the
/// build decides what it can be, and the flag only narrows it.
fn choose_profile(debug: bool, release: bool, debug_build: bool) -> Result<Profile, String> {
    if debug && release {
        return Err("`--debug` and `--release` are mutually exclusive".to_string());
    }
    if debug {
        if !debug_build {
            return Err("this is a release build; `--debug` is not available".to_string());
        }
        return Ok(Profile::Debug);
    }
    if release {
        return Ok(Profile::Release);
    }
    Ok(if debug_build {
        Profile::Debug
    } else {
        Profile::Release
    })
}

/// Reads what a boot would read, without creating or binding anything. This is
/// the whole of the doctor command: a daemon that is not running can still be
/// configured wrong.
async fn check(config_path: PathBuf, profile: Profile) -> Result<(), Error> {
    let config = Config::load_with(&config_path, profile)?;
    config.validate(profile)?;
    match anth_engine::state::check(&config.state)? {
        anth_engine::state::StateStatus::Missing => {
            println!(
                "ok: no state at {} yet; the first boot creates it",
                config.state.display()
            );
            return Ok(());
        }
        anth_engine::state::StateStatus::Ready => {}
    }
    let composition = anth_engine::state::load_composition(&config.state)?;
    composition.validate()?;
    println!(
        "ok: configuration, state and composition are valid ({} apps composed)",
        composition.profile[0].apps.len()
    );
    Ok(())
}

async fn run(args: Args) -> Result<(), Error> {
    let config = Config::load_or_create(&args.config, args.profile)?;
    config.validate(args.profile)?;
    let logger = Logger::new(config.log.clone());

    if config.services.allow_third_party {
        logger.warn(
            "`services.allow-third-party` is on: third-party bundles may declare native services",
        );
    }
    if args.reset_credentials && anth_engine::state::reset_credentials(&config.state)? {
        logger.warn("operator credentials reset; a new password will be generated");
    }
    anth_engine::state::ensure(&config.state)?;
    let composition = anth_engine::state::load_composition(&config.state)?;
    composition.validate()?;
    let auth = Arc::new(anth_auth::Auth::load_or_create(
        &config.state,
        config.auth.session_ttl.0,
    )?);
    logger.info(&format!(
        "operator credentials at {}",
        auth.credentials_path().display()
    ));
    logger.info(&format!(
        "state ready at {} (apps, grants, keys, composition, audit)",
        config.state.display()
    ));

    // SAFETY: `geteuid` has no preconditions.
    let allowed_uid = unsafe { libc::geteuid() };
    let runtime = Arc::new(WasmtimeRuntime::new()?);
    let supervisor = Arc::new(Supervisor::new(runtime));
    let services = Arc::new(anth_engine::service::ServiceSupervisor::for_state(
        &config.state,
        std::time::Duration::from_millis(config.limits.wall_ms),
    ));
    let (stop, stopped) = watch::channel(None::<Stop>);
    let state = Arc::new(
        anth_api::EngineState::new(allowed_uid)
            .with_composition(composition)
            .allow_degraded(config.composition.allow_degraded)
            .with_apps(
                &config.state,
                config.apps.max_bundle_bytes,
                config.apps.require_signature,
                config.services.allow_third_party,
            )
            .with_caller(Arc::new(anth_engine::service::EngineCaller::new(
                anth_engine::supervisor::SupervisorCaller::new(supervisor.clone()),
                services.clone(),
            )))
            .with_restart(Arc::new(RestartProcess { stop: stop.clone() }))
            .with_auth(auth)
            .with_service_uid(Arc::new(|id: &anth_contract::AppId| {
                anth_engine::sandbox::allocate_uid(id.as_str()).ok()
            }))
            .booting(),
    );
    let socket = anth_engine::socket::bind(&config.socket, &logger).await?;
    anth_engine::socket::restrict(&config.socket)?;
    logger.info(&format!("listening on {}", config.socket.display()));

    let final_stop = stopped.clone();
    tokio::spawn(async move {
        shutdown_signal().await;
        let _ = stop.send(Some(Stop::Shutdown));
    });

    // Serve before starting apps: a slow app must be diagnosable, and readiness
    // answers `503 starting` until the boot finishes.
    let router_state = Arc::clone(&state);
    let uds_stopped = stopped.clone();
    let uds = tokio::spawn(async move {
        axum::serve(
            socket,
            anth_api::router(router_state)
                .into_make_service_with_connect_info::<anth_api::PeerUid>(),
        )
        .with_graceful_shutdown(wait(uds_stopped))
        .await
    });

    let public = anth_api::public_router(Arc::clone(&state));
    let tcp = match &config.listen {
        Some(address) => {
            let address: SocketAddr = address
                .parse()
                .map_err(|_| Error::invalid(format!("`{address}` is not a socket address")))?;
            let listener = bind_tcp(address)
                .map_err(|error| Error::internal(format!("binding {address}: {error}")))?;
            logger.info(&format!("listening on http://{address}"));
            let stopped = stopped.clone();
            Some(tokio::spawn(async move {
                axum::serve(
                    listener,
                    public.into_make_service_with_connect_info::<SocketAddr>(),
                )
                .with_graceful_shutdown(wait(stopped))
                .await
            }))
        }
        None => None,
    };

    if let Err(error) = state.start_brokers() {
        logger.warn(&format!(
            "the broker did not start: {}: {}; serving so it can be repaired",
            error.kind().code(),
            error.message()
        ));
    }
    let composition = state.composition.read().expect("composition").clone();
    let report = anth_engine::boot::start_composed(
        &supervisor,
        &services,
        &config.state,
        &composition,
        anth_engine::boot::BootOptions {
            allow_degraded: config.composition.allow_degraded,
            allow_third_party_service: config.services.allow_third_party,
            limits: &config.limits,
            log: LogSettings {
                level: config.log.level,
                format: config.log.format,
            },
        },
    )
    .await?;
    if let Some(error) = &report.fatal {
        logger.warn(&format!(
            "the composition cannot boot: {}: {}; serving so it can be repaired",
            error.kind().code(),
            error.message()
        ));
    }
    for (app, reason) in &report.degraded {
        logger.warn(&format!("degraded {app}: {reason}"));
    }
    logger.info(&format!(
        "started {} app(s), {} degraded",
        report.started.len(),
        report.degraded.len()
    ));
    state.mark_ready();

    uds.await
        .map_err(|error| Error::internal(format!("serving the API: {error}")))?
        .map_err(|error| Error::internal(format!("serving the API: {error}")))?;
    if let Some(task) = tcp {
        task.await
            .map_err(|error| Error::internal(format!("serving the API: {error}")))?
            .map_err(|error| Error::internal(format!("serving the API: {error}")))?;
    }
    services.stop_all().await;
    supervisor.stop_all().await;
    if *final_stop.borrow() == Some(Stop::Restart) {
        logger.info("restarting");
        return reexec();
    }
    logger.info("stopped");
    Ok(())
}

/// Why serving stopped: a signal, or a host-requested restart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stop {
    Shutdown,
    Restart,
}

/// The engine's claim on its own process: the router only asks, and the daemon
/// replaces itself with the same arguments after an orderly stop.
struct RestartProcess {
    stop: watch::Sender<Option<Stop>>,
}

impl anth_api::Restart for RestartProcess {
    fn restart(&self) -> Result<(), Error> {
        // Once a stop is in progress a restart no longer wins: a service
        // manager's TERM must not be turned into a re-exec by a late request.
        let applied = self.stop.send_if_modified(|value| {
            if value.is_some() {
                return false;
            }
            *value = Some(Stop::Restart);
            true
        });
        if applied {
            Ok(())
        } else {
            Err(Error::unavailable("the engine is already stopping"))
        }
    }
}

/// Replaces this process with the same arguments, so the pid and the service
/// manager's contract survive the restart.
fn reexec() -> Result<(), Error> {
    use std::os::unix::process::CommandExt;
    let exe = std::env::current_exe()
        .map_err(|error| Error::internal(format!("locating this binary: {error}")))?;
    let error = std::process::Command::new(exe)
        .args(std::env::args_os().skip(1))
        .exec();
    Err(Error::internal(format!("re-exec failed: {error}")))
}

/// Binds with `SO_REUSEADDR`: a restart replaces the process while connections
/// from the old one can still be in `TIME_WAIT`, and the port must be reusable.
fn bind_tcp(address: SocketAddr) -> std::io::Result<TcpListener> {
    let socket = socket2::Socket::new(
        socket2::Domain::for_address(address),
        socket2::Type::STREAM,
        Some(socket2::Protocol::TCP),
    )?;
    socket.set_reuse_address(true)?;
    socket.bind(&address.into())?;
    socket.listen(1024)?;
    socket.set_nonblocking(true)?;
    TcpListener::from_std(socket.into())
}

/// Resolves when the shutdown signal fires.
async fn wait(mut stopped: watch::Receiver<Option<Stop>>) {
    loop {
        if stopped.borrow().is_some() {
            return;
        }
        if stopped.changed().await.is_err() {
            return;
        }
    }
}

/// Ctrl-C or SIGTERM, whichever comes first.
async fn shutdown_signal() {
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
    use super::{choose_profile, Args, RestartProcess, Stop};
    use anth_api::Restart;
    use anth_engine::config::Profile;

    #[test]
    fn a_restart_request_reaches_the_stop_channel() {
        let (stop, stopped) = tokio::sync::watch::channel(None::<Stop>);
        let process = RestartProcess { stop };
        process.restart().expect("restart");
        assert_eq!(*stopped.borrow(), Some(Stop::Restart));
    }

    #[test]
    fn a_stop_in_progress_cannot_become_a_restart() {
        let (stop, stopped) = tokio::sync::watch::channel(Some(Stop::Shutdown));
        let process = RestartProcess { stop };
        let error = process.restart().expect_err("already stopping");
        assert!(error.message().contains("stopping"), "{error}");
        assert_eq!(*stopped.borrow(), Some(Stop::Shutdown));
    }

    #[test]
    fn a_release_build_cannot_be_forced_open() {
        let error = choose_profile(true, false, false).expect_err("no debug in release");
        assert!(error.contains("release build"), "{error}");
    }

    #[test]
    fn the_default_follows_the_build() {
        assert_eq!(choose_profile(false, false, true), Ok(Profile::Debug));
        assert_eq!(choose_profile(false, false, false), Ok(Profile::Release));
    }

    #[test]
    fn the_flags_narrow_the_default() {
        assert_eq!(choose_profile(false, true, true), Ok(Profile::Release));
        assert_eq!(choose_profile(true, false, true), Ok(Profile::Debug));
        assert!(choose_profile(true, true, true).is_err());
    }

    #[test]
    fn resetting_the_credentials_is_explicit() {
        let args = Args::parse(std::iter::empty()).expect("args");
        assert!(!args.reset_credentials);
        let args =
            Args::parse(["--reset-credentials"].into_iter().map(str::to_string)).expect("args");
        assert!(args.reset_credentials);
        assert!(Args::parse(
            ["--check", "--reset-credentials"]
                .into_iter()
                .map(str::to_string)
        )
        .is_err());
    }

    #[test]
    fn reset_removes_the_file_and_is_idempotent() {
        let dir = std::env::temp_dir().join("anthracite-panel").join(format!(
            "anth-engine-reset-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join(anth_auth::CREDENTIALS);
        std::fs::write(&path, b"credentials").expect("write");
        assert!(anth_engine::state::reset_credentials(&dir).expect("reset"));
        assert!(!path.exists());
        assert!(!anth_engine::state::reset_credentials(&dir).expect("idempotent"));
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
}
