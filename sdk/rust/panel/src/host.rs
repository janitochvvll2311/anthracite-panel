//! The linked host: boot the engine and mount its router in the panel process.
//!
//! This is the v0.5.0 panel (`D-Remote-Impl`): the panel links the SDK, the SDK
//! links the engine, and the browser speaks to one process. The Unix socket is
//! still bound for the CLI, with peer credentials, while the browser uses the
//! engine's session cookie.

use std::path::Path;
use std::sync::Arc;

use anth_api::{EngineState, PeerUid, SessionInfo};
use anth_contract::Error;
use anth_engine::config::{Config, Profile};
use anth_engine::log::Logger;
use anth_engine::supervisor::{Supervisor, SupervisorCaller};
use anth_runtime::{LogSettings, WasmtimeRuntime};
use axum::Router;
use tokio::sync::watch;
use tokio::task::JoinHandle;

/// A cloneable handle to the engine's session store, for the panel's own shell
/// authentication. It does not expose the credentials.
#[derive(Clone)]
pub struct Sessions {
    auth: Arc<anth_api::Auth>,
}

impl Sessions {
    pub fn authenticate(&self, token: &str) -> Option<SessionInfo> {
        self.auth.authenticate(token)
    }
}

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
        Self::start_with_reset(config_path, false).await
    }

    /// Same boot, optionally deleting the recovery file first. The panel exposes
    /// this as `--reset-credentials`, matching the daemon.
    pub async fn start_with_reset(config_path: &Path, reset: bool) -> Result<Self, Error> {
        let profile = Profile::default();
        let config = Config::load_or_create(config_path, profile)?;
        config.validate(profile)?;
        let logger = Logger::new(config.log.clone());

        if reset && anth_engine::state::reset_credentials(&config.state)? {
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
        let state = Arc::new(
            EngineState::new(allowed_uid)
                .with_composition(composition.clone())
                .with_apps(
                    &config.state,
                    config.apps.max_bundle_bytes,
                    config.apps.require_signature,
                )
                .with_caller(Arc::new(SupervisorCaller::new(supervisor.clone())))
                .with_auth(auth)
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

    /// The `/api` surface over engine sessions. Browser requests carry the
    /// session cookie; the panel does not inject a trusted marker.
    pub fn session_api(&self) -> Router {
        anth_api::session_router(Arc::clone(&self.state))
    }

    /// The session store the panel's shell middleware reads.
    pub fn sessions(&self) -> Option<Sessions> {
        self.state.auth.as_ref().map(|auth| Sessions {
            auth: Arc::clone(auth),
        })
    }

    /// Stops the socket and every running app.
    pub async fn stop(self) {
        let _ = self.stop.send(true);
        let _ = self.socket.await;
        self.supervisor.stop_all().await;
    }
}
