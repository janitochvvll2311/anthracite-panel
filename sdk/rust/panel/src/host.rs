//! The linked host: boot the engine and mount its router in the panel process.
//!
//! This is the v0.5.0 panel (`D-Remote-Impl`): the panel links the SDK, the SDK
//! links the engine, and the browser speaks to one process. The Unix socket is
//! still bound for the CLI, with peer credentials, while the browser uses the
//! engine's session cookie.

use std::path::Path;
use std::sync::Arc;

use anth_api::{EngineState, PeerUid, Restart, SessionInfo};
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

/// Why the host stopped: a signal, or a restart the API asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    Shutdown,
    Restart,
}

/// A booted engine owned by the panel.
pub struct Host {
    state: Arc<EngineState>,
    supervisor: Arc<Supervisor>,
    services: Arc<anth_engine::service::ServiceSupervisor>,
    stop: watch::Sender<Option<Stop>>,
    socket: JoinHandle<std::io::Result<()>>,
}

/// The panel's claim on its own process: the router asks, the binary replaces
/// itself after the host stops (§6.5).
struct RestartProcess {
    stop: watch::Sender<Option<Stop>>,
}

impl Restart for RestartProcess {
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
            Err(Error::unavailable("the panel is already stopping"))
        }
    }
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

        if config.services.allow_third_party {
            logger.warn(
                "`services.allow-third-party` is on: third-party bundles may declare native services",
            );
        }
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
        let services = Arc::new(anth_engine::service::ServiceSupervisor::for_state(
            &config.state,
            std::time::Duration::from_millis(config.limits.wall_ms),
        ));
        let (stop, stopped) = watch::channel(None::<Stop>);
        let state = Arc::new(
            EngineState::new(allowed_uid)
                .with_composition(composition.clone())
                .with_apps(
                    &config.state,
                    config.apps.max_bundle_bytes,
                    config.apps.require_signature,
                    config.services.allow_third_party,
                )
                .with_caller(Arc::new(anth_engine::service::EngineCaller::new(
                    SupervisorCaller::new(supervisor.clone()),
                    services.clone(),
                )))
                .with_restart(Arc::new(RestartProcess { stop: stop.clone() }))
                .with_auth(auth)
                .booting(),
        );

        let socket = anth_engine::socket::bind(&config.socket, &logger).await?;
        anth_engine::socket::restrict(&config.socket)?;
        logger.info(&format!("listening on {}", config.socket.display()));

        let router_state = Arc::clone(&state);
        let socket_task = tokio::spawn(async move {
            let mut stopped = stopped;
            axum::serve(
                socket,
                anth_api::router(router_state).into_make_service_with_connect_info::<PeerUid>(),
            )
            .with_graceful_shutdown(async move {
                while stopped.borrow().is_none() {
                    if stopped.changed().await.is_err() {
                        break;
                    }
                }
            })
            .await
        });

        let report = anth_engine::boot::start_composed(
            &supervisor,
            &services,
            &config.state,
            &composition,
            config.services.allow_third_party,
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
                let _ = stop.send(Some(Stop::Shutdown));
                let _ = socket_task.await;
                services.stop_all().await;
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
            services,
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

    /// A receiver the panel's own server watches so it stops accepting when the
    /// API asks to restart the process. A plain shutdown is `Some(Shutdown)`.
    pub fn restart_signal(&self) -> watch::Receiver<Option<Stop>> {
        self.stop.subscribe()
    }

    /// Stops the socket and every running app. `reason` is decided by the
    /// caller: a signal means shutdown, the API's restart signal means re-exec.
    pub async fn stop(self, reason: Stop) -> Stop {
        let _ = self.stop.send(Some(reason));
        let _ = self.socket.await;
        self.services.stop_all().await;
        self.supervisor.stop_all().await;
        reason
    }
}
