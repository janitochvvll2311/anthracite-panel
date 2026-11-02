//! The engine daemon: read the configuration, prepare the state, serve the API.
//!
//! A headless deployment runs this binary; the official panel links the same
//! crates and mounts the same router in its own process (§6.5).

mod config;
mod log;
mod socket;
mod state;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;

use anth_contract::Error;
use config::{Config, Profile};
use log::Logger;
use tokio::net::TcpListener;
use tokio::sync::watch;

struct Args {
    config: PathBuf,
    profile: Profile,
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
        .and_then(|runtime| runtime.block_on(run(args)))
    {
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
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--config" => {
                    config = Some(args.next().ok_or("`--config` needs a path")?);
                }
                "--debug" => debug = true,
                "--release" => release = true,
                "--help" | "-h" => {
                    return Err(
                        "usage: anth-engine [--config <path>] [--debug|--release]".to_string()
                    );
                }
                other => return Err(format!("unknown argument `{other}`")),
            }
        }
        Ok(Self {
            config: PathBuf::from(config.unwrap_or_else(|| "config.toml".to_string())),
            profile: choose_profile(debug, release, cfg!(debug_assertions))?,
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

async fn run(args: Args) -> Result<(), Error> {
    let config = Config::load_or_create(&args.config, args.profile)?;
    config.validate(args.profile)?;
    let logger = Logger::new(config.log.clone());

    state::ensure(&config.state)?;
    logger.info(&format!(
        "state ready at {} (apps, grants, keys, composition, audit)",
        config.state.display()
    ));

    let socket = socket::bind(&config.socket, &logger).await?;
    socket::restrict(&config.socket)?;
    logger.info(&format!("listening on {}", config.socket.display()));

    let router = anth_api::router(anth_api::state());
    let (stop, stopped) = watch::channel(false);
    tokio::spawn(async move {
        shutdown_signal().await;
        let _ = stop.send(true);
    });

    let tcp = match &config.listen {
        Some(address) => {
            let address: SocketAddr = address
                .parse()
                .map_err(|_| Error::invalid(format!("`{address}` is not a socket address")))?;
            let listener = TcpListener::bind(address)
                .await
                .map_err(|error| Error::internal(format!("binding {address}: {error}")))?;
            logger.info(&format!("listening on http://{address}"));
            let router = router.clone();
            let stopped = stopped.clone();
            Some(tokio::spawn(async move {
                axum::serve(listener, router)
                    .with_graceful_shutdown(wait(stopped))
                    .await
            }))
        }
        None => None,
    };

    axum::serve(socket, router)
        .with_graceful_shutdown(wait(stopped))
        .await
        .map_err(|error| Error::internal(format!("serving the API: {error}")))?;

    if let Some(task) = tcp {
        let _ = task.await;
    }
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
    use crate::config::Profile;

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
