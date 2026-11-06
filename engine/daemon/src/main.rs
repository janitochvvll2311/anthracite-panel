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
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--config" => {
                    config = Some(args.next().ok_or("`--config` needs a path")?);
                }
                "--debug" => debug = true,
                "--release" => release = true,
                "--check" => check = true,
                "--help" | "-h" => {
                    return Err(
                        "usage: anth-engine [--config <path>] [--debug|--release] [--check]"
                            .to_string(),
                    );
                }
                other => return Err(format!("unknown argument `{other}`")),
            }
        }
        Ok(Self {
            config: PathBuf::from(config.unwrap_or_else(|| "config.toml".to_string())),
            profile: choose_profile(debug, release, cfg!(debug_assertions))?,
            check,
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
    let config = Config::load(&config_path)?;
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

    anth_engine::state::ensure(&config.state)?;
    let composition = anth_engine::state::load_composition(&config.state)?;
    composition.validate()?;
    logger.info(&format!(
        "state ready at {} (apps, grants, keys, composition, audit)",
        config.state.display()
    ));

    // SAFETY: `geteuid` has no preconditions.
    let allowed_uid = unsafe { libc::geteuid() };
    let state = Arc::new(
        anth_api::EngineState::new(allowed_uid)
            .with_composition(composition)
            .with_apps(
                &config.state,
                config.apps.max_bundle_bytes,
                config.apps.require_signature,
            )
            .booting(),
    );
    let socket = anth_engine::socket::bind(&config.socket, &logger).await?;
    anth_engine::socket::restrict(&config.socket)?;
    logger.info(&format!("listening on {}", config.socket.display()));

    let (stop, stopped) = watch::channel(false);
    tokio::spawn(async move {
        shutdown_signal().await;
        let _ = stop.send(true);
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

    let public = anth_api::public_router();
    let tcp = match &config.listen {
        Some(address) => {
            let address: SocketAddr = address
                .parse()
                .map_err(|_| Error::invalid(format!("`{address}` is not a socket address")))?;
            let listener = TcpListener::bind(address)
                .await
                .map_err(|error| Error::internal(format!("binding {address}: {error}")))?;
            logger.info(&format!("listening on http://{address}"));
            let stopped = stopped.clone();
            Some(tokio::spawn(async move {
                axum::serve(listener, public)
                    .with_graceful_shutdown(wait(stopped))
                    .await
            }))
        }
        None => None,
    };

    let runtime = Arc::new(WasmtimeRuntime::new()?);
    let supervisor = Supervisor::new(runtime);
    let composition = state.composition.read().expect("composition").clone();
    let report = anth_engine::boot::start_composed(
        &supervisor,
        &config.state,
        &composition,
        &config.limits,
        LogSettings {
            level: config.log.level,
            format: config.log.format,
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
    // The supervisor owns the running instances; it must outlive the boot call.
    let supervisor = supervisor;

    uds.await
        .map_err(|error| Error::internal(format!("serving the API: {error}")))?
        .map_err(|error| Error::internal(format!("serving the API: {error}")))?;
    if let Some(task) = tcp {
        task.await
            .map_err(|error| Error::internal(format!("serving the API: {error}")))?
            .map_err(|error| Error::internal(format!("serving the API: {error}")))?;
    }
    supervisor.stop_all().await;
    logger.info("stopped");
    Ok(())
}

/// Resolves when the shutdown signal fires.
async fn wait(mut stopped: watch::Receiver<bool>) {
    loop {
        if *stopped.borrow() {
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
    use super::choose_profile;
    use anth_engine::config::Profile;

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
}
