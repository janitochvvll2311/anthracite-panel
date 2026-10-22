use super::prelude::*;

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
