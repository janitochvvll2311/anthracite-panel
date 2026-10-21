use std::{
    error::Error,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use crate::auth::session::{AuthService, SharedAuth};
use crate::config::Config;
use anth_proto::AnthClient;
use tokio::net::UnixStream;
use tokio::sync::RwLock;

/// State shared by every HTTP request.
///
/// The configuration lives behind a lock because the settings endpoints write
/// it: `PATCH /api/config` has to change the in-memory copy *and* the file in
/// one step, and a request that read the config at boot would otherwise keep
/// serving the old policy until a restart.
#[derive(Clone, Debug)]
pub struct AppState {
    config: Arc<RwLock<Config>>,
    auth: Arc<RwLock<SharedAuth>>,
    /// Mirror of `files.max_upload_bytes`. The axum body limit is a router
    /// constant, so it needs the value before any request can await the lock.
    max_upload_bytes: Arc<AtomicU64>,
    /// Where `save` writes. `None` when the caller built the state directly,
    /// which is what the tests do: they never touch `/etc`.
    config_path: Option<Arc<Path>>,
}

impl AppState {
    /// Builds state from an in-memory config. Changes are not persisted.
    pub fn new(config: Config) -> Self {
        let auth = Arc::new(AuthService::from_config(&config.auth));
        let max_upload_bytes = Arc::new(AtomicU64::new(config.files.max_upload_bytes));
        Self {
            config: Arc::new(RwLock::new(config)),
            auth: Arc::new(RwLock::new(auth)),
            max_upload_bytes,
            config_path: None,
        }
    }

    /// Builds state that can persist configuration changes to `path`.
    pub fn with_config_path(config: Config, path: impl Into<Arc<Path>>) -> Self {
        let auth = Arc::new(AuthService::from_config(&config.auth));
        let max_upload_bytes = Arc::new(AtomicU64::new(config.files.max_upload_bytes));
        Self {
            config: Arc::new(RwLock::new(config)),
            auth: Arc::new(RwLock::new(auth)),
            max_upload_bytes,
            config_path: Some(path.into()),
        }
    }

    /// The upload ceiling, readable without awaiting.
    pub fn max_upload_bytes(&self) -> u64 {
        self.max_upload_bytes.load(Ordering::Relaxed)
    }

    /// The current configuration.
    pub async fn config(&self) -> Config {
        self.config.read().await.clone()
    }

    /// The live auth service.
    pub async fn auth(&self) -> SharedAuth {
        Arc::clone(&*self.auth.read().await)
    }

    /// Replaces the configuration without touching the live sessions.
    ///
    /// The auth *policy* is applied in place: the same `Arc<AuthService>` keeps
    /// serving, so a settings save does not sign the operator out. Rebuilding
    /// the service is reserved for a credential change, which is
    /// [`Self::replace_auth`].
    pub async fn replace_config(&self, config: Config) {
        self.max_upload_bytes
            .store(config.files.max_upload_bytes, Ordering::Relaxed);
        self.auth.write().await.apply_config(&config.auth);
        *self.config.write().await = config;
    }

    /// Swaps in a new auth service, ending every session it held.
    ///
    /// A password change calls this: the new secret invalidates the access
    /// tokens already handed out, and the empty session map invalidates the
    /// refresh tokens with them.
    pub async fn replace_auth(&self, config: Config) {
        let auth = Arc::new(AuthService::from_config(&config.auth));
        self.max_upload_bytes
            .store(config.files.max_upload_bytes, Ordering::Relaxed);
        *self.auth.write().await = auth;
        *self.config.write().await = config;
    }

    /// Writes the current configuration to disk, when a path is known.
    pub async fn persist(&self) -> Result<(), String> {
        let Some(path) = self.config_path.clone() else {
            return Ok(());
        };
        let config = self.config().await;
        config.save(path.as_ref())
    }

    /// Opens one connection to the daemon. Callers that need independent read
    /// and write halves (the terminal bridge) use this directly.
    pub async fn connect_stream(&self) -> Result<UnixStream, Box<dyn Error + Send + Sync>> {
        let config = self.config().await;
        let socket_path = Path::new(&config.socket_path);
        match UnixStream::connect(socket_path).await {
            Ok(stream) => Ok(stream),
            Err(e) => {
                eprintln!("Failed to connect to socket: {e}");
                Err(Box::new(e))
            }
        }
    }

    pub async fn create_client(&self) -> Result<AnthClient, Box<dyn Error + Send + Sync>> {
        Ok(AnthClient::new(self.connect_stream().await?))
    }
}
