//! The linked host: boot the engine and mount its router in the panel process.
//!
//! This is the v0.5.0 panel (`D-Remote-Impl`): the panel links the SDK, the SDK
//! links the engine, and the browser speaks to one process. The Unix socket is
//! still bound for the CLI, with peer credentials, while the panel marks its
//! own browser requests `Trusted` (sessions will replace that blanket trust).

use std::path::Path;
use std::sync::Arc;

use anth_api::{EngineState, PeerUid, Trusted};
use anth_contract::Error;
use anth_engine::config::{Config, Profile};
use anth_engine::log::Logger;
use anth_engine::supervisor::{Supervisor, SupervisorCaller};
use anth_runtime::{LogSettings, WasmtimeRuntime};
use axum::{Extension, Router};
use tokio::sync::watch;
use tokio::task::JoinHandle;

/// A booted engine owned by the panel.
pub struct Host {
    state: Arc<EngineState>,
    supervisor: Arc<Supervisor>,
    stop: watch::Sender<bool>,
    socket: JoinHandle<std::io::Result<()>>,
}

impl Host {
    /// Reads the configuration, creates state on first boot, starts the
    /// composition and binds both the Unix socket and the API state.
    pub async fn start(config_path: &Path) -> Result<Self, Error> {
        let profile = Profile::default();
        let config = Config::load_or_create(config_path, profile)?;
        config.validate(profile)?;
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
        let runtime = Arc::new(WasmtimeRuntime::new()?);
        let supervisor = Arc::new(Supervisor::new(runtime));
        let state = Arc::new(
            EngineState::new(allowed_uid)
                .with_composition(composition.clone())
                .with_apps(
                    &config.state,
                    config.apps.max_bundle_bytes,
                    config.apps.require_signature,
                )
                .with_caller(Arc::new(SupervisorCaller::new(supervisor.clone())))
                .booting(),
        );

        let socket = anth_engine::socket::bind(&config.socket, &logger).await?;
        anth_engine::socket::restrict(&config.socket)?;
        logger.info(&format!("listening on {}", config.socket.display()));

        let (stop, stopped) = watch::channel(false);
        let router_state = Arc::clone(&state);
        let socket_task = tokio::spawn(async move {
            let mut stopped = stopped;
            axum::serve(
                socket,
                anth_api::router(router_state).into_make_service_with_connect_info::<PeerUid>(),
            )
            .with_graceful_shutdown(async move {
                while !*stopped.borrow() {
                    if stopped.changed().await.is_err() {
                        break;
                    }
                }
            })
            .await
        });

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
        .await;
        let report = match report {
            Ok(report) => report,
            Err(error) => {
                let _ = stop.send(true);
                let _ = socket_task.await;
                supervisor.stop_all().await;
                return Err(error);
            }
        };
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

        Ok(Self {
            state,
            supervisor,
            stop,
            socket: socket_task,
        })
    }

    /// The `/api` surface, ready to nest. Browser requests reach the router
    /// without a peer credential, so the host marks them trusted; the router
    /// still owns what a trusted request may reach.
    pub fn api(&self) -> Router {
        anth_api::api_router(Arc::clone(&self.state)).layer(Extension(Trusted))
    }

    /// Stops the socket and every running app.
    pub async fn stop(self) {
        let _ = self.stop.send(true);
        let _ = self.socket.await;
        self.supervisor.stop_all().await;
    }
}
