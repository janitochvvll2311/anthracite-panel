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
        let mut profile = None;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--config" => {
                    config = Some(args.next().ok_or("`--config` needs a path")?);
                }
                "--debug" => profile = Some(Profile::Debug),
                "--release" => profile = Some(Profile::Release),
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
            profile: profile.unwrap_or_default(),
        })
    }
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
