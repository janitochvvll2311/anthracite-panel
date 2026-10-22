use super::guards::*;
use super::prelude::*;
use futures::StreamExt;
use std::os::unix::fs::OpenOptionsExt;
use tokio::io::AsyncWriteExt;

/// Body of `GET /api/plugins`.
///
/// Read-only and derived from the bundle directory, so it does not travel over
/// the socket: the daemon has no opinion about which plugins the panel shows.
#[derive(Debug, Serialize)]
pub struct PluginsView {
    pub enabled: bool,
    pub install_from_web: bool,
    pub plugins: Vec<PluginView>,
    /// Plugins that failed to load, for the settings view to display.
    pub problems: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct PluginView {
    pub id: String,
    pub version: String,
    pub name: String,
    pub enabled: bool,
    pub capabilities: Vec<String>,
    pub api: Vec<crate::plugins::ApiAnchor>,
    pub ui: Vec<crate::plugins::UiAnchor>,
}

pub async fn get_plugins(State(state): State<AppState>) -> Result<Json<PluginsView>, ApiError> {
    let config = state.config().await;
    // Loading walks the plugin root and hashes every module, up to 32 MiB each.
    // That is blocking work, so it goes to a worker rather than stalling the
    // runtime for the duration of the hashing a settings page triggers on poll.
    let plugins = config.plugins.clone();
    let registry =
        tokio::task::spawn_blocking(move || crate::plugins::PluginRegistry::load(&plugins))
            .await
            .map_err(|error| ApiError::internal(format!("plugin scan failed: {error}")))?;

    Ok(Json(PluginsView {
        enabled: config.plugins.enabled,
        install_from_web: config.plugins.install_from_web,
        plugins: registry
            .plugins()
            .iter()
            .map(|plugin| PluginView {
                id: plugin.id.clone(),
                version: plugin.version.clone(),
                name: plugin.name.clone(),
                enabled: plugin.enabled,
                capabilities: plugin.capabilities.clone(),
                api: plugin.api.clone(),
                ui: plugin.ui.clone(),
            })
            .collect(),
        problems: registry.problems().to_vec(),
    }))
}

/// Body of a successful `POST /api/plugins/install`.
#[derive(Debug, Serialize)]
pub struct PluginInstalledView {
    pub id: String,
    pub version: String,
    pub name: String,
    /// Present when the bundle was accepted without a verified signature.
    pub warning: Option<String>,
}

/// `POST /api/plugins/install` — install one bundle from the browser.
///
/// The body is the zip itself, not a multipart form: the CLI takes a path and
/// the browser has a `File`, and both become the same `install_bundle` call. The
/// route is gated by `[plugins] install_from_web`, because installing means
/// running code on the host.
pub async fn post_plugin_install(
    State(state): State<AppState>,
    body: Body,
) -> Result<Json<PluginInstalledView>, ApiError> {
    require_plugin_install(&state).await?;

    let mut config = state.config().await;
    if !config.plugins.enabled {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "plugins are disabled in the configuration",
        ));
    }

    let mut upload = Upload::create()?;
    write_upload(body, upload.take_file()).await?;

    let plugins = config.plugins.clone();
    let path = upload.path.clone();
    let result =
        tokio::task::spawn_blocking(move || crate::plugins::install_bundle(&path, &plugins))
            .await
            .map_err(|error| {
                ApiError::internal(format!("plugin install worker failed: {error}"))
            })?;
    // The temporary zip is no longer needed whether the install succeeded or
    // not; dropping `upload` below handles it as well, but remove it now so a
    // long-running process does not keep the bytes around.
    drop(upload);

    let result = result.map_err(plugin_install_error)?;
    let id = result.installed.plugin.id.clone();
    let version = result.installed.plugin.version.clone();
    let name = result.installed.plugin.name.clone();
    config.plugins.upsert_grant(result.grant);
    state.replace_config(config).await;
    state.persist().await.map_err(ApiError::internal)?;

    Ok(Json(PluginInstalledView {
        id,
        version,
        name,
        warning: result.warning,
    }))
}

/// Temporary file that removes itself on drop.
struct Upload {
    path: std::path::PathBuf,
    file: Option<std::fs::File>,
}

impl Upload {
    fn create() -> Result<Self, ApiError> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "anth-plugin-upload-{}-{nanos}.zip",
            std::process::id()
        ));
        // `create_new` plus `O_NOFOLLOW` keeps a pre-planted symlink or an
        // existing file from being written through. The handle is kept and
        // written directly, so the path cannot be swapped between create and
        // open either.
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)
            .map_err(|error| ApiError::internal(format!("could not create the upload: {error}")))?;
        Ok(Self {
            path,
            file: Some(file),
        })
    }

    fn take_file(&mut self) -> std::fs::File {
        self.file
            .take()
            .expect("the upload file is taken exactly once")
    }
}

impl Drop for Upload {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

async fn write_upload(body: Body, file: std::fs::File) -> Result<(), ApiError> {
    let mut file = tokio::fs::File::from_std(file);
    let mut stream = body.into_data_stream();
    let mut received = 0u64;

    while let Some(frame) = stream.next().await {
        let bytes = frame.map_err(|error| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                format!("could not read the bundle: {error}"),
            )
        })?;
        received = received
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "bundle size overflow"))?;
        if received > crate::plugins::MAX_BUNDLE_BYTES {
            return Err(ApiError::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                format!(
                    "the bundle is larger than {} bytes",
                    crate::plugins::MAX_BUNDLE_BYTES
                ),
            ));
        }
        file.write_all(&bytes)
            .await
            .map_err(|error| ApiError::internal(format!("could not write the upload: {error}")))?;
    }

    file.flush()
        .await
        .map_err(|error| ApiError::internal(format!("could not flush the upload: {error}")))
}

fn plugin_install_error(error: crate::plugins::PluginError) -> ApiError {
    use crate::plugins::PluginErrorKind;

    let status = match error.kind {
        PluginErrorKind::Denied => StatusCode::FORBIDDEN,
        PluginErrorKind::Invalid | PluginErrorKind::Tampered => StatusCode::BAD_REQUEST,
        PluginErrorKind::Missing => StatusCode::NOT_FOUND,
        PluginErrorKind::Conflict => StatusCode::CONFLICT,
        PluginErrorKind::Io => StatusCode::INTERNAL_SERVER_ERROR,
    };
    ApiError::new(status, error.detail)
}
