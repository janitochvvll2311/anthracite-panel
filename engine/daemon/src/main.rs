//! The engine daemon: read the configuration, prepare the state, serve the API.
//!
//! A headless deployment runs this binary; the official panel links the same
//! crates and mounts the same router in its own process (§6.5).

mod config;
mod log;
mod state;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anth_contract::Error;
use config::{Config, Profile};
use log::Logger;
use tokio::net::UnixListener;

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
        let mut profile = Profile::Release;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--config" => {
                    config = Some(args.next().ok_or("`--config` needs a path")?);
                }
                "--debug" => profile = Profile::Debug,
                "--help" | "-h" => {
                    return Err("usage: anth-engine [--config <path>] [--debug]".to_string());
                }
                other => return Err(format!("unknown argument `{other}`")),
            }
        }
        Ok(Self {
            config: PathBuf::from(config.unwrap_or_else(|| "config.toml".to_string())),
            profile,
        })
    }
}

async fn run(args: Args) -> Result<(), Error> {
    let config = Config::load_or_create(&args.config, args.profile)?;
    config.validate()?;
    let logger = Logger::new(config.log.clone());

    state::ensure(&config.state)?;
    logger.info(&format!(
        "state ready at {} (apps, grants, keys, composition, audit)",
        config.state.display()
    ));

    let listener = bind(&config.socket, &logger).await?;
    logger.info(&format!("listening on {}", config.socket.display()));

    let state = anth_api::state();
    axum::serve(listener, anth_api::router(state))
        .with_graceful_shutdown(shutdown())
        .await
        .map_err(|error| Error::internal(format!("serving the API: {error}")))?;

    logger.info("stopped");
    Ok(())
}

/// Binds the socket, replacing a stale one but never a live one.
async fn bind(path: &Path, logger: &Logger) -> Result<UnixListener, Error> {
    if let Ok(metadata) = std::fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() {
            return Err(Error::invalid(format!(
                "{} is a symlink; the socket must be a real file",
                path.display()
            )));
        }
        match tokio::net::UnixStream::connect(path).await {
            Ok(_) => {
                return Err(Error::conflict(format!(
                    "another engine is listening on {}",
                    path.display()
                )));
            }
            Err(_) => {
                logger.warn(&format!("removing the stale socket {}", path.display()));
                std::fs::remove_file(path).map_err(|error| {
                    Error::internal(format!(
                        "removing the stale socket {}: {error}",
                        path.display()
                    ))
                })?;
            }
        }
    }
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .map_err(|error| Error::internal(format!("creating {}: {error}", parent.display())))?;
    }
    UnixListener::bind(path)
        .map_err(|error| Error::internal(format!("binding {}: {error}", path.display())))
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
