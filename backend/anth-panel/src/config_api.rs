//! `GET /api/config` and `PATCH /api/config`.
//!
//! The settings view edits the same sections `config set` accepts, but through
//! JSON so a form can send several changes at once. Every field is a full
//! replacement rather than a deep merge: an absent key keeps the current value,
//! a present key replaces it. That keeps a partial update predictable, which a
//! merge over arrays and nested tables is not.
//!
//! Secrets never travel outward. The response reports whether a password is set
//! and the signing key's presence, never the values, so the settings page cannot
//! leak what only the panel needs.

use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};

use crate::api::ApiError;
use crate::app::AppState;
use crate::config::Config;

/// The editable view of the configuration.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConfigView {
    pub socket_path: String,
    pub sudo: bool,
    pub terminal: TerminalView,
    pub process: ProcessView,
    pub network: NetworkView,
    pub firewall: FirewallView,
    pub volumes: VolumesView,
    pub accounts: AccountsView,
    pub files: FilesView,
    pub auth: AuthView,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TerminalView {
    pub shell: String,
    pub max_sessions: usize,
    /// Whether a terminal token is configured; the token itself is never sent.
    pub token_set: bool,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessView {
    pub control: bool,
    pub spawn: bool,
    pub spawn_allow: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct NetworkView {
    pub control: bool,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FirewallView {
    pub control: bool,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct VolumesView {
    pub control: bool,
    pub mount_root: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccountsView {
    pub control: bool,
    pub manage_passwords: bool,
    pub allow_system: bool,
    pub allow_privileged_groups: bool,
    pub home_root: String,
    pub default_shell: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FilesView {
    pub root: String,
    pub write: bool,
    pub max_entries: usize,
    pub max_preview_bytes: u64,
    pub max_upload_bytes: u64,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthView {
    pub enabled: bool,
    pub username: String,
    /// Whether a password digest is stored.
    pub password_set: bool,
    pub access_token_secs: u64,
    pub refresh_token_secs: u64,
    pub cookie_secure: bool,
}

impl ConfigView {
    pub fn from_config(config: &Config) -> Self {
        Self {
            socket_path: config.socket_path.clone(),
            sudo: config.sudo,
            terminal: TerminalView {
                shell: config.terminal.shell.clone(),
                max_sessions: config.terminal.max_sessions,
                token_set: !config.terminal.token.is_empty(),
            },
            process: ProcessView {
                control: config.process.control,
                spawn: config.process.spawn,
                spawn_allow: config.process.spawn_allow.clone(),
            },
            network: NetworkView {
                control: config.network.control,
            },
            firewall: FirewallView {
                control: config.firewall.control,
            },
            volumes: VolumesView {
                control: config.volumes.control,
                mount_root: config.volumes.mount_root.clone(),
            },
            accounts: AccountsView {
                control: config.accounts.control,
                manage_passwords: config.accounts.manage_passwords,
                allow_system: config.accounts.allow_system,
                allow_privileged_groups: config.accounts.allow_privileged_groups,
                home_root: config.accounts.home_root.clone(),
                default_shell: config.accounts.default_shell.clone(),
            },
            files: FilesView {
                root: config.files.root.clone(),
                write: config.files.write,
                max_entries: config.files.max_entries,
                max_preview_bytes: config.files.max_preview_bytes,
                max_upload_bytes: config.files.max_upload_bytes,
            },
            auth: AuthView {
                enabled: config.auth.enabled,
                username: config.auth.username.clone(),
                password_set: !config.auth.password_hash.is_empty(),
                access_token_secs: config.auth.access_token_secs,
                refresh_token_secs: config.auth.refresh_token_secs,
                cookie_secure: config.auth.cookie_secure,
            },
        }
    }
}

/// A patch. Each section is optional; a present section replaces its fields.
///
/// `auth` deliberately has no password field: a change there goes through
/// `POST /api/auth/password`, which checks the current password and revokes
/// every session.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ConfigPatch {
    pub socket_path: Option<String>,
    pub sudo: Option<bool>,
    pub terminal: Option<TerminalPatch>,
    pub process: Option<ProcessPatch>,
    pub network: Option<NetworkPatch>,
    pub firewall: Option<FirewallPatch>,
    pub volumes: Option<VolumesPatch>,
    pub accounts: Option<AccountsPatch>,
    pub files: Option<FilesPatch>,
    pub auth: Option<AuthPatch>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TerminalPatch {
    pub shell: Option<String>,
    pub max_sessions: Option<usize>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProcessPatch {
    pub control: Option<bool>,
    pub spawn: Option<bool>,
    pub spawn_allow: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NetworkPatch {
    pub control: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FirewallPatch {
    pub control: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct VolumesPatch {
    pub control: Option<bool>,
    pub mount_root: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AccountsPatch {
    pub control: Option<bool>,
    pub manage_passwords: Option<bool>,
    pub allow_system: Option<bool>,
    pub allow_privileged_groups: Option<bool>,
    pub home_root: Option<String>,
    pub default_shell: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FilesPatch {
    pub root: Option<String>,
    pub write: Option<bool>,
    pub max_entries: Option<usize>,
    pub max_preview_bytes: Option<u64>,
    pub max_upload_bytes: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AuthPatch {
    pub enabled: Option<bool>,
    pub username: Option<String>,
    pub access_token_secs: Option<u64>,
    pub refresh_token_secs: Option<u64>,
    pub cookie_secure: Option<bool>,
}

/// `GET /api/config`.
pub async fn get_config(State(state): State<AppState>) -> Json<ConfigView> {
    Json(ConfigView::from_config(&state.config().await))
}

/// `PATCH /api/config` — applies a patch, then saves the file.
///
/// The new config is validated by round-tripping through TOML before it is
/// stored: the loader would reject a bad file on the next start, and finding out
/// then would leave the panel running on a config it cannot reload.
pub async fn patch_config(
    State(state): State<AppState>,
    payload: Result<Json<ConfigPatch>, axum::extract::rejection::JsonRejection>,
) -> Result<Json<ConfigView>, ApiError> {
    let Json(patch) = payload.map_err(|error| ApiError::bad_request(error.body_text()))?;

    let mut config = state.config().await;
    apply_patch(&mut config, patch).map_err(ApiError::bad_request)?;

    let text = config
        .to_toml()
        .map_err(|error| ApiError::internal(error.to_string()))?;
    crate::config::Config::from_toml(&text)
        .map_err(|error| ApiError::bad_request(format!("invalid configuration: {error}")))?;

    state.replace_config(config).await;
    state.persist().await.map_err(ApiError::internal)?;

    Ok(Json(ConfigView::from_config(&state.config().await)))
}

fn apply_patch(config: &mut Config, patch: ConfigPatch) -> Result<(), String> {
    if let Some(socket_path) = patch.socket_path {
        if socket_path.trim().is_empty() {
            return Err("socket_path must not be empty".to_string());
        }
        config.socket_path = socket_path;
    }
    if let Some(sudo) = patch.sudo {
        config.sudo = sudo;
    }
    if let Some(terminal) = patch.terminal {
        if let Some(shell) = terminal.shell {
            config.terminal.shell = shell;
        }
        if let Some(max_sessions) = terminal.max_sessions {
            if max_sessions == 0 {
                return Err("terminal.max_sessions must be positive".to_string());
            }
            config.terminal.max_sessions = max_sessions;
        }
    }
    if let Some(process) = patch.process {
        if let Some(control) = process.control {
            config.process.control = control;
        }
        if let Some(spawn) = process.spawn {
            config.process.spawn = spawn;
        }
        if let Some(spawn_allow) = process.spawn_allow {
            // An entry that is not absolute can never match, so accepting it
            // would store a list that silently denies part of itself.
            if let Some(entry) = spawn_allow.iter().find(|entry| !entry.starts_with('/')) {
                return Err(format!(
                    "process.spawn_allow entries must be absolute paths, got `{entry}`"
                ));
            }
            config.process.spawn_allow = spawn_allow;
        }
    }
    if let Some(control) = patch.network.and_then(|network| network.control) {
        config.network.control = control;
    }
    if let Some(control) = patch.firewall.and_then(|firewall| firewall.control) {
        config.firewall.control = control;
    }
    if let Some(volumes) = patch.volumes {
        if let Some(control) = volumes.control {
            config.volumes.control = control;
        }
        if let Some(mount_root) = volumes.mount_root {
            if !mount_root.starts_with('/') {
                return Err("volumes.mount_root must be an absolute path".to_string());
            }
            config.volumes.mount_root = mount_root;
        }
    }
    if let Some(accounts) = patch.accounts {
        if let Some(control) = accounts.control {
            config.accounts.control = control;
        }
        if let Some(manage_passwords) = accounts.manage_passwords {
            config.accounts.manage_passwords = manage_passwords;
        }
        if let Some(allow_system) = accounts.allow_system {
            config.accounts.allow_system = allow_system;
        }
        if let Some(allow_privileged_groups) = accounts.allow_privileged_groups {
            config.accounts.allow_privileged_groups = allow_privileged_groups;
        }
        if let Some(home_root) = accounts.home_root {
            if !home_root.starts_with('/') {
                return Err("accounts.home_root must be an absolute path".to_string());
            }
            config.accounts.home_root = home_root;
        }
        if let Some(default_shell) = accounts.default_shell {
            config.accounts.default_shell = default_shell;
        }
    }
    if let Some(files) = patch.files {
        if let Some(root) = files.root {
            if !root.starts_with('/') {
                return Err("files.root must be an absolute path".to_string());
            }
            config.files.root = root;
        }
        if let Some(write) = files.write {
            config.files.write = write;
        }
        if let Some(max_entries) = files.max_entries {
            if max_entries == 0 {
                return Err("files.max_entries must be positive".to_string());
            }
            config.files.max_entries = max_entries;
        }
        if let Some(max_preview_bytes) = files.max_preview_bytes {
            config.files.max_preview_bytes = max_preview_bytes;
        }
        if let Some(max_upload_bytes) = files.max_upload_bytes {
            config.files.max_upload_bytes = max_upload_bytes;
        }
    }
    if let Some(auth) = patch.auth {
        if auth.enabled == Some(false) {
            return Err(
                "authentication cannot be disabled from the panel; use `anth-panel auth enable --disable` on the host"
                    .to_string(),
            );
        }
        if let Some(enabled) = auth.enabled {
            config.auth.enabled = enabled;
        }
        if let Some(username) = auth.username {
            if username.trim().is_empty() {
                return Err("auth.username must not be empty".to_string());
            }
            config.auth.username = username;
        }
        if let Some(access) = auth.access_token_secs {
            if access == 0 {
                return Err("auth.access_token_secs must be positive".to_string());
            }
            config.auth.access_token_secs = access;
        }
        if let Some(refresh) = auth.refresh_token_secs {
            if refresh == 0 {
                return Err("auth.refresh_token_secs must be positive".to_string());
            }
            config.auth.refresh_token_secs = refresh;
        }
        if let Some(cookie_secure) = auth.cookie_secure {
            config.auth.cookie_secure = cookie_secure;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patch(json: &str) -> Result<ConfigPatch, serde_json::Error> {
        serde_json::from_str(json)
    }

    #[test]
    fn a_view_never_carries_a_secret() {
        let mut config = Config::default();
        config.auth.password_hash = "pbkdf2-sha256$1$a$b".to_string();
        config.terminal.token = "super-secret".to_string();

        let text = serde_json::to_string(&ConfigView::from_config(&config)).unwrap();
        assert!(!text.contains("super-secret"));
        assert!(!text.contains("pbkdf2-sha256"));
        assert!(text.contains("\"token_set\":true"));
        assert!(text.contains("\"password_set\":true"));
    }

    #[test]
    fn an_absent_key_keeps_the_current_value() {
        let mut config = Config::default();
        config.files.max_entries = 12;
        apply_patch(&mut config, patch(r#"{"files":{"write":true}}"#).unwrap()).unwrap();
        assert_eq!(config.files.max_entries, 12);
        assert!(config.files.write);
    }

    #[test]
    fn a_relative_root_is_refused() {
        let mut config = Config::default();
        let error =
            apply_patch(&mut config, patch(r#"{"files":{"root":"srv"}}"#).unwrap()).unwrap_err();
        assert!(error.contains("absolute"));
    }

    #[test]
    fn auth_cannot_be_disabled_from_the_panel() {
        let mut config = Config::default();
        let error =
            apply_patch(&mut config, patch(r#"{"auth":{"enabled":false}}"#).unwrap()).unwrap_err();
        assert!(error.contains("auth enable --disable"));
    }

    #[test]
    fn process_spawn_and_its_allowlist_are_patchable() {
        let mut config = Config::default();

        apply_patch(
            &mut config,
            patch(r#"{"process":{"spawn":true,"spawn_allow":["/bin/echo"]}}"#).unwrap(),
        )
        .unwrap();

        assert!(config.process.spawn);
        assert_eq!(config.process.spawn_allow, vec!["/bin/echo".to_string()]);
        // The signal switch is a separate concern and keeps its value.
        assert!(!config.process.control);
    }

    #[test]
    fn a_relative_spawn_allow_entry_is_refused() {
        // Accepting it would store a list that silently denies part of itself,
        // because the check compares absolute paths.
        let mut config = Config::default();
        let error = apply_patch(
            &mut config,
            patch(r#"{"process":{"spawn_allow":["echo"]}}"#).unwrap(),
        )
        .unwrap_err();

        assert!(error.contains("absolute"));
        assert!(config.process.spawn_allow.is_empty());
    }

    #[test]
    fn the_process_view_reports_the_spawn_settings() {
        let mut config = Config::default();
        config.process.spawn = true;
        config.process.spawn_allow = vec!["/bin/echo".to_string()];

        let view = ConfigView::from_config(&config);

        assert!(view.process.spawn);
        assert_eq!(view.process.spawn_allow, vec!["/bin/echo".to_string()]);
    }

    #[test]
    fn an_unknown_section_is_refused_by_the_deserializer() {
        assert!(patch(r#"{"files":{"unknown":1}}"#).is_err());
    }
}
