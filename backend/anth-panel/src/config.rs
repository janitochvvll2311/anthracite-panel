//! Operator policy: the config file, its defaults and its path mapping.
//!
//! `config.toml` holds policy only. Credentials live in `secrets.toml`, session
//! records in `<state_dir>/sessions.toml`, and extension grants under
//! `<config_dir>/extensions.d`. Every write goes through [`crate::state`] so a
//! comment or key order is never lost.

use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use toml_edit::{DocumentMut, Item, Table};

use crate::log::{Format, Level, Out};
use crate::state;

pub const CONFIG_VERSION: u32 = 1;
pub const DEFAULT_CONFIG_FILE: &str = "config.toml";
pub const RELEASE_CONFIG_PATH: &str = "/etc/anth/config.toml";
pub const RELEASE_SOCKET_PATH: &str = "/var/tmp/anth.sock";
pub const RELEASE_STATE_DIR: &str = "/var/lib/anth";
pub const RELEASE_APP_ROOT: &str = "/var/lib/anth/apps";
pub const SECRETS_FILE: &str = "secrets.toml";
pub const APP_GRANTS_DIR: &str = "apps.d";
pub const APP_STATE_FILE: &str = "apps.state.toml";
pub const SESSIONS_FILE: &str = "sessions.toml";

/// First-run credential mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum BootstrapMode {
    /// Debug resolves to `web`, release to `generated`.
    #[default]
    Auto,
    /// Generate a password and print it once to stderr.
    Generated,
    /// Open the registration form/route until the first password is set.
    Web,
}

impl BootstrapMode {
    /// Resolves `auto` for the build profile.
    pub fn resolve(self) -> Self {
        match self {
            BootstrapMode::Auto if cfg!(debug_assertions) => BootstrapMode::Web,
            BootstrapMode::Auto => BootstrapMode::Generated,
            explicit => explicit,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServerConfig {
    pub addr: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            addr: "0.0.0.0:8080".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LogConfig {
    pub level: Level,
    pub format: Format,
    pub out: Out,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            level: Level::Info,
            format: Format::Human,
            out: Out::Stdout,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TerminalConfig {
    pub enabled: bool,
    pub shell: String,
    pub max_sessions: usize,
    pub idle_seconds: u64,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            shell: "auto".to_string(),
            max_sessions: 4,
            idle_seconds: 900,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ProcessConfig {
    pub control: bool,
    pub spawn: bool,
    pub spawn_allow: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct NetworkConfig {
    pub control: bool,
    pub manage: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct FirewallConfig {
    pub control: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct VolumesConfig {
    pub control: bool,
    pub partition: bool,
    pub format: bool,
    pub mount_root: String,
    pub disk_allow: Vec<String>,
}

impl VolumesConfig {
    /// Host-side policy for the storage backend.
    pub fn settings(&self) -> anth_host::volumes::VolumesSettings {
        anth_host::volumes::VolumesSettings {
            control: self.control,
            partition: self.partition,
            format: self.format,
            mount_root: self.mount_root.clone(),
            disk_allow: self.disk_allow.clone(),
        }
    }
}

impl Default for VolumesConfig {
    fn default() -> Self {
        Self {
            control: false,
            partition: false,
            format: false,
            mount_root: "/mnt".to_string(),
            disk_allow: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AccountsConfig {
    pub control: bool,
    pub manage_passwords: bool,
    pub allow_system: bool,
    pub allow_privileged_groups: bool,
    pub home_root: String,
    pub shells: Vec<String>,
    pub default_shell: String,
}

impl AccountsConfig {
    /// Host-side policy for the account backend.
    pub fn settings(&self) -> anth_host::accounts::AccountsSettings {
        anth_host::accounts::AccountsSettings {
            control: self.control,
            manage_passwords: self.manage_passwords,
            allow_system: self.allow_system,
            allow_privileged_groups: self.allow_privileged_groups,
            home_root: self.home_root.clone(),
            shells: self.shells.clone(),
            default_shell: self.default_shell.clone(),
        }
    }
}

impl Default for AccountsConfig {
    fn default() -> Self {
        Self {
            control: false,
            manage_passwords: false,
            allow_system: false,
            allow_privileged_groups: false,
            home_root: "/home".to_string(),
            shells: Vec::new(),
            default_shell: "/bin/sh".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FilesConfig {
    pub root: String,
    pub write: bool,
    pub trash: bool,
    pub max_entries: usize,
    pub max_preview_bytes: u64,
    pub max_upload_bytes: u64,
    pub max_trash_bytes: u64,
    pub max_archive_bytes: u64,
}

impl Default for FilesConfig {
    fn default() -> Self {
        Self {
            root: "/".to_string(),
            write: false,
            trash: false,
            max_entries: 5_000,
            max_preview_bytes: 1024 * 1024,
            max_upload_bytes: 64 * 1024 * 1024,
            max_trash_bytes: 1024 * 1024 * 1024,
            max_archive_bytes: 4 * 1024 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AuthConfig {
    pub enabled: bool,
    pub username: String,
    pub access_token_secs: u64,
    pub refresh_token_secs: u64,
    pub cookie_secure: bool,
    pub bootstrap: BootstrapMode,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            username: "admin".to_string(),
            access_token_secs: 15 * 60,
            refresh_token_secs: 30 * 24 * 60 * 60,
            cookie_secure: false,
            bootstrap: BootstrapMode::Auto,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AppsConfig {
    pub enabled: bool,
    pub root: String,
    pub install_from_web: bool,
    pub require_signature: bool,
    pub public_key: String,
    pub fuel: u64,
}

impl Default for AppsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            root: RELEASE_APP_ROOT.to_string(),
            install_from_web: false,
            require_signature: true,
            public_key: String::new(),
            fuel: 50_000_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DemoConfig {
    /// Empty means every demo feature. Otherwise only these names are on:
    /// `network`, `firewall`, `files`, `processes`, `volumes`,
    /// `accounts` and `terminal`.
    pub features: Vec<String>,
    /// Idle lifetime of one anonymous demo session; `0` disables expiry.
    pub session_ttl_seconds: u64,
    /// Upper bound on live demo sessions before the least recently used one is
    /// evicted.
    pub max_sessions: usize,
}

impl Default for DemoConfig {
    fn default() -> Self {
        Self {
            features: Vec::new(),
            session_ttl_seconds: 3600,
            max_sessions: 64,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub version: u32,
    pub socket_path: String,
    pub state_dir: String,
    pub server: ServerConfig,
    pub log: LogConfig,
    pub terminal: TerminalConfig,
    pub process: ProcessConfig,
    pub network: NetworkConfig,
    pub firewall: FirewallConfig,
    pub volumes: VolumesConfig,
    pub accounts: AccountsConfig,
    pub files: FilesConfig,
    pub auth: AuthConfig,
    pub apps: AppsConfig,
    pub demo: DemoConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            socket_path: RELEASE_SOCKET_PATH.to_string(),
            state_dir: RELEASE_STATE_DIR.to_string(),
            server: ServerConfig::default(),
            log: LogConfig::default(),
            terminal: TerminalConfig::default(),
            process: ProcessConfig::default(),
            network: NetworkConfig::default(),
            firewall: FirewallConfig::default(),
            volumes: VolumesConfig::default(),
            accounts: AccountsConfig::default(),
            files: FilesConfig::default(),
            auth: AuthConfig::default(),
            apps: AppsConfig::default(),
            demo: DemoConfig::default(),
        }
    }
}

impl Config {
    /// Release-safe defaults.
    pub fn server_default() -> Self {
        if cfg!(debug_assertions) {
            Self::dev_default()
        } else {
            Self::default()
        }
    }

    /// Local defaults: relative paths and every policy switch on, including the
    /// sensitive ones, so a debug run can exercise every feature. Release builds
    /// never see this profile.
    ///
    /// `disk_allow` stays empty on purpose: it is the list of devices the
    /// operator is willing to repartition, not a policy switch, and the panel
    /// must not guess one. Add a throwaway disk before testing partition edits.
    pub fn dev_default() -> Self {
        let mut config = Self {
            socket_path: "./anth.sock".to_string(),
            state_dir: "./state".to_string(),
            ..Self::default()
        };
        config.terminal.enabled = true;
        config.process.control = true;
        config.process.spawn = true;
        config.process.spawn_allow = vec![
            "/bin/echo".to_string(),
            "/bin/sleep".to_string(),
            "/usr/bin/uptime".to_string(),
        ];
        config.network.control = true;
        config.network.manage = true;
        config.firewall.control = true;
        config.volumes.control = true;
        config.volumes.partition = true;
        config.volumes.format = true;
        config.accounts.control = true;
        config.accounts.manage_passwords = true;
        config.accounts.allow_system = true;
        config.accounts.allow_privileged_groups = true;
        config.apps.enabled = true;
        config.apps.root = "./apps/bundles".to_string();
        config.apps.install_from_web = true;
        config.apps.require_signature = false;
        config.files.write = true;
        config.files.trash = true;
        config
    }

    /// The default config path for this build.
    pub fn default_config_path() -> PathBuf {
        if cfg!(debug_assertions) {
            PathBuf::from("./").join(DEFAULT_CONFIG_FILE)
        } else {
            PathBuf::from(RELEASE_CONFIG_PATH)
        }
    }

    /// Reads and validates a config. A missing file is an error; use
    /// [`Config::load_or_create`] for the server path.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        let text = fs::read_to_string(path)
            .map_err(|error| format!("reading {}: {error}", path.display()))?;
        Self::from_toml(&text).map_err(|error| format!("{}: {error}", path.display()))
    }

    /// Parses and validates config text.
    pub fn from_toml(text: &str) -> Result<Self, String> {
        let raw: toml::Value =
            toml::from_str(text).map_err(|error| format!("the config does not parse: {error}"))?;
        match raw.get("version").and_then(toml::Value::as_integer) {
            Some(version) if version == i64::from(CONFIG_VERSION) => {}
            Some(version) => {
                return Err(format!(
                    "unsupported config version {version}; run `anth-panel migrate`"
                ));
            }
            None => {
                return Err(
                    "the config has no `version`; run `anth-panel migrate` for a v0.2 file"
                        .to_string(),
                );
            }
        }

        let config: Self =
            toml::from_str(text).map_err(|error| format!("the config is invalid: {error}"))?;
        config.validate()?;
        Ok(config)
    }

    /// Loads a config, creating it from defaults when the file is missing.
    pub fn load_or_create(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        match fs::read_to_string(path) {
            Ok(text) => {
                Self::from_toml(&text).map_err(|error| format!("{}: {error}", path.display()))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let config = Self::server_default();
                config.write_template(path)?;
                Ok(config)
            }
            Err(error) => Err(format!("reading {}: {error}", path.display())),
        }
    }

    /// Loads a config without writing or creating anything.
    pub fn load_or_default(path: impl AsRef<Path>) -> Self {
        Self::load(path).unwrap_or_else(|error| {
            eprintln!("{error}");
            Self::server_default()
        })
    }

    /// Basic cross-field validation.
    pub fn validate(&self) -> Result<(), String> {
        if self.version != CONFIG_VERSION {
            return Err(format!(
                "unsupported config version {}; expected {CONFIG_VERSION}",
                self.version
            ));
        }
        if self.socket_path.trim().is_empty() {
            return Err("`socket_path` must not be empty".to_string());
        }
        if self.state_dir.trim().is_empty() {
            return Err("`state_dir` must not be empty".to_string());
        }
        if self.server.addr.trim().is_empty() {
            return Err("`server.addr` must not be empty".to_string());
        }
        if self.auth.access_token_secs == 0 {
            return Err("`auth.access_token_secs` must be greater than zero".to_string());
        }
        if self.auth.refresh_token_secs == 0 {
            return Err("`auth.refresh_token_secs` must be greater than zero".to_string());
        }
        if self.auth.username.trim().is_empty() {
            return Err("`auth.username` must not be empty".to_string());
        }
        if self.terminal.max_sessions == 0 {
            return Err("`terminal.max_sessions` must be greater than zero".to_string());
        }
        if self.files.max_entries == 0 {
            return Err("`files.max_entries` must be greater than zero".to_string());
        }
        if self.files.max_preview_bytes == 0
            || self.files.max_upload_bytes == 0
            || self.files.max_archive_bytes == 0
        {
            return Err("file size limits must be greater than zero".to_string());
        }
        if self.apps.fuel == 0 {
            return Err("`extensions.fuel` must be greater than zero".to_string());
        }
        Ok(())
    }

    /// The path mapping derived from this config and its own file path.
    pub fn paths(&self, config_path: impl AsRef<Path>) -> Result<Paths, String> {
        let config_path = config_path.as_ref();
        let config_dir = config_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        let resolve = |value: &str| -> PathBuf {
            let path = PathBuf::from(value);
            if path.is_absolute() {
                return path;
            }
            let mut out = config_dir.clone();
            for component in path.components() {
                match component {
                    Component::CurDir => {}
                    Component::ParentDir => {
                        out.pop();
                    }
                    Component::Normal(part) => out.push(part),
                    Component::RootDir | Component::Prefix(_) => {
                        // A relative value cannot contribute a root.
                    }
                }
            }
            out
        };

        let state_dir = resolve(&self.state_dir);
        let apps = resolve(&self.apps.root);
        Ok(Paths {
            config: config_path.to_path_buf(),
            config_dir: config_dir.clone(),
            socket: resolve(&self.socket_path),
            state_dir: state_dir.clone(),
            secrets: config_dir.join(SECRETS_FILE),
            sessions: state_dir.join(SESSIONS_FILE),
            apps,
            grants: config_dir.join(APP_GRANTS_DIR),
            app_state: state_dir.join(APP_STATE_FILE),
        })
    }

    /// The bootstrap mode after resolving `auto` for this build.
    pub fn bootstrap(&self) -> BootstrapMode {
        self.auth.bootstrap.resolve()
    }

    /// Changes one dotted key, preserving comments and key order.
    ///
    /// The new text is parsed and validated before the atomic write, so a bad
    /// value leaves the original file untouched.
    pub fn set_key(path: impl AsRef<Path>, key: &str, raw_value: &str) -> Result<(), String> {
        let path = path.as_ref();
        state::update(path, 0o644, |current| {
            let mut document = if current.trim().is_empty() {
                Self::server_default().template().parse::<DocumentMut>()
            } else {
                current.parse::<DocumentMut>()
            }
            .map_err(|error| format!("the config does not parse: {error}"))?;

            let snippet = format!("value = {raw_value}\n");
            let parsed = snippet
                .parse::<DocumentMut>()
                .map_err(|error| format!("`{raw_value}` is not a TOML value: {error}"))?;
            let item = parsed
                .get("value")
                .cloned()
                .ok_or_else(|| "the TOML value is missing".to_string())?;
            set_dotted(&mut document, key, item)?;

            let text = document.to_string();
            Self::from_toml(&text)?;
            Ok(text)
        })
    }

    /// A commented template for a new config file.
    pub fn template(&self) -> String {
        let body = toml::to_string_pretty(self).expect("the config serializes");
        format!(
            "# Anthracite Panel configuration, version {CONFIG_VERSION}.\n\
             # Edits preserve comments and key order. A file that will not parse is refused.\n\
             # Secrets, sessions and extension grants live beside this file, not in it.\n\
             #\n{body}"
        )
    }

    fn write_template(&self, path: &Path) -> Result<(), String> {
        state::write_atomic(path, self.template().as_bytes(), 0o644)
    }
}

/// Resolved filesystem paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub config: PathBuf,
    pub config_dir: PathBuf,
    pub socket: PathBuf,
    pub state_dir: PathBuf,
    pub secrets: PathBuf,
    pub sessions: PathBuf,
    pub apps: PathBuf,
    pub grants: PathBuf,
    pub app_state: PathBuf,
}

fn set_dotted(document: &mut DocumentMut, key: &str, item: Item) -> Result<(), String> {
    let parts: Vec<&str> = key.split('.').collect();
    if parts.iter().any(|part| part.is_empty()) {
        return Err(format!("`{key}` is not a dotted key"));
    }
    let (last, parents) = parts.split_last().expect("at least one part");
    let mut table = document.as_table_mut();
    for parent in parents {
        let entry = table
            .entry(parent)
            .or_insert_with(|| Item::Table(Table::new()));
        table = entry
            .as_table_mut()
            .ok_or_else(|| format!("`{parent}` is not a table"))?;
    }
    table.insert(last, item);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = PathBuf::from("/tmp/anthracite-panel/tests")
            .join(format!("{name}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&root).expect("create scratch");
        root
    }

    #[test]
    fn the_default_template_round_trips() {
        let config = Config::server_default();
        let text = config.template();
        let parsed = Config::from_toml(&text).expect("template parses");
        assert_eq!(parsed.version, CONFIG_VERSION);
        assert_eq!(parsed.auth.username, "admin");
    }

    #[test]
    fn debug_defaults_are_relative_and_release_defaults_are_absolute() {
        let dev = Config::dev_default();
        assert_eq!(dev.socket_path, "./anth.sock");
        assert_eq!(dev.state_dir, "./state");
        assert_eq!(dev.apps.root, "./apps/bundles");

        let release = Config::default();
        assert_eq!(release.socket_path, RELEASE_SOCKET_PATH);
        assert_eq!(release.state_dir, RELEASE_STATE_DIR);
        assert_eq!(release.apps.root, RELEASE_APP_ROOT);
    }

    #[test]
    fn the_dev_profile_enables_every_policy_switch() {
        let dev = Config::dev_default();
        assert!(dev.terminal.enabled);
        assert!(dev.process.control);
        assert!(dev.process.spawn);
        assert!(!dev.process.spawn_allow.is_empty());
        assert!(dev.network.control);
        assert!(dev.network.manage);
        assert!(dev.firewall.control);
        assert!(dev.volumes.control);
        assert!(dev.volumes.partition);
        assert!(dev.volumes.format);
        assert!(dev.accounts.control);
        assert!(dev.accounts.manage_passwords);
        assert!(dev.accounts.allow_system);
        assert!(dev.accounts.allow_privileged_groups);
        assert!(dev.apps.enabled);
        assert!(dev.apps.install_from_web);
        assert!(!dev.apps.require_signature);
        assert!(dev.files.write);
        assert!(dev.files.trash);
        assert!(
            dev.volumes.disk_allow.is_empty(),
            "the device allowlist stays an explicit operator choice"
        );

        let release = Config::default();
        assert!(!release.process.spawn);
        assert!(!release.network.manage);
        assert!(!release.volumes.partition);
        assert!(!release.volumes.format);
        assert!(!release.accounts.manage_passwords);
        assert!(!release.accounts.allow_system);
        assert!(release.apps.require_signature);
        assert!(!release.files.write);
    }

    #[test]
    fn a_missing_version_is_refused() {
        let text = Config::default()
            .template()
            .lines()
            .filter(|line| !line.starts_with("version ="))
            .collect::<Vec<_>>()
            .join("\n");
        let error = Config::from_toml(&text).expect_err("refused");
        assert!(error.contains("no `version`"));
    }

    #[test]
    fn an_unknown_field_is_refused() {
        let mut text = Config::default().template();
        text.push_str("\n[unknown]\nvalue = 1\n");
        let error = Config::from_toml(&text).expect_err("refused");
        assert!(error.contains("unknown"));
    }

    #[test]
    fn paths_resolve_against_the_config_directory() {
        let config = Config::dev_default();
        let paths = config.paths("/srv/anth/config.toml").expect("paths");
        assert_eq!(paths.socket, PathBuf::from("/srv/anth/anth.sock"));
        assert_eq!(paths.state_dir, PathBuf::from("/srv/anth/state"));
        assert_eq!(paths.secrets, PathBuf::from("/srv/anth/secrets.toml"));
        assert_eq!(paths.grants, PathBuf::from("/srv/anth/apps.d"));
        assert_eq!(
            paths.sessions,
            PathBuf::from("/srv/anth/state/sessions.toml")
        );
    }

    #[test]
    fn set_key_preserves_comments_and_round_trips() {
        let root = scratch("config-set");
        let path = root.join("config.toml");
        let mut text = Config::dev_default().template();
        text.push_str("\n# keep this comment\n");
        fs::write(&path, text).expect("write");
        Config::set_key(&path, "files.write", "true").expect("set");
        let updated = fs::read_to_string(&path).unwrap();
        assert!(updated.contains("# keep this comment"));
        assert!(updated.contains("write = true"));
        Config::load(&path).expect("valid after edit");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_bad_value_leaves_the_file_untouched() {
        let root = scratch("config-bad");
        let path = root.join("config.toml");
        fs::write(&path, Config::dev_default().template()).expect("write");
        let before = fs::read_to_string(&path).unwrap();
        let error = Config::set_key(&path, "auth.username", "\"unterminated").expect_err("refused");
        assert!(error.contains("not a TOML value"));
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
        fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    fn text() -> impl Strategy<Value = String> {
        proptest::collection::vec(any::<char>(), 0..1024)
            .prop_map(|chars| chars.into_iter().collect())
    }

    proptest! {
        #[test]
        fn arbitrary_config_text_never_panics(text in text()) {
            let _ = Config::from_toml(&text);
        }
    }
}
