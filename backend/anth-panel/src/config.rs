use serde::{Deserialize, Serialize};
use std::{error::Error, path::Path};

pub const DEFAULT_CONFIG_PATH: &str = "/etc/anth/config.toml";
pub const DEFAULT_SOCKET_PATH: &str = "/var/tmp/anth.sock";

/// Web terminal settings. The token is reserved for the pending auth layer.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct TerminalConfig {
    /// Program to spawn for each session. `"auto"` uses `$SHELL` when it points
    /// at an existing file, then falls back to `/bin/sh`.
    pub shell: String,
    /// Shared token reserved for the authentication layer. Not enforced yet.
    pub token: String,
    /// Maximum number of concurrent sessions across the daemon.
    pub max_sessions: usize,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            shell: "auto".to_string(),
            // Reserved for the authentication layer. A terminal is remote code
            // execution, so the token stays empty until that layer exists.
            token: String::new(),
            max_sessions: 4,
        }
    }
}

impl TerminalConfig {
    /// Resolves the configured shell, following `$SHELL` (then the account's
    /// login shell) for `"auto"`.
    pub fn resolved_shell(&self) -> String {
        let login = login_shell();
        resolve_shell(
            &self.shell,
            std::env::var("SHELL").ok().as_deref(),
            login.as_deref(),
        )
    }
}

fn resolve_shell(configured: &str, environment: Option<&str>, login: Option<&str>) -> String {
    if configured != "auto" && !configured.is_empty() {
        return configured.to_string();
    }

    environment
        .filter(|shell| !shell.is_empty() && Path::new(shell).is_file())
        .or_else(|| login.filter(|shell| !shell.is_empty() && Path::new(shell).is_file()))
        .unwrap_or("/bin/sh")
        .to_string()
}

/// The current account's login shell from `/etc/passwd`, if it is usable.
fn login_shell() -> Option<String> {
    let uid = unsafe { libc::getuid() };
    let passwd = std::fs::read_to_string("/etc/passwd").ok()?;
    shell_from_passwd(&passwd, uid)
}

fn shell_from_passwd(text: &str, uid: u32) -> Option<String> {
    text.lines().find_map(|line| {
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() < 7 || fields[2].parse::<u32>().ok() != Some(uid) {
            return None;
        }

        let shell = fields[6];
        if shell.is_empty() || !Path::new(shell).is_file() {
            return None;
        }
        Some(shell.to_string())
    })
}

/// Process monitoring and control settings.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ProcessConfig {
    /// Whether the daemon accepts signal requests. Off by default: killing a
    /// process is destructive and the panel has no authentication yet.
    pub control: bool,
}

/// Network interface monitoring and control settings.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(default, deny_unknown_fields)]
pub struct NetworkConfig {
    /// Whether the daemon accepts up/down requests. Off by default: bringing an
    /// interface down can disconnect the host, and the panel has no
    /// authentication yet.
    pub control: bool,
}

/// Firewall management settings.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(default, deny_unknown_fields)]
pub struct FirewallConfig {
    /// Whether the daemon accepts rule changes. Off by default: the panel has
    /// no authentication yet and firewall changes can disconnect the host.
    pub control: bool,
}

/// Filesystem explorer settings.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct FilesConfig {
    /// Host directory exposed as `/`. Must be absolute and exist when the
    /// daemon starts.
    pub root: String,
    /// Whether the daemon accepts mkdir, rename, delete and upload requests.
    /// Off by default: the panel has no authentication yet.
    pub write: bool,
    /// Maximum number of entries returned by one listing.
    pub max_entries: usize,
    /// Maximum bytes returned by the inline content endpoint.
    pub max_preview_bytes: u64,
    /// Maximum size of one upload.
    pub max_upload_bytes: u64,
}

impl Default for FilesConfig {
    fn default() -> Self {
        Self {
            root: "/".to_string(),
            write: false,
            max_entries: 5000,
            max_preview_bytes: 1024 * 1024,
            max_upload_bytes: 64 * 1024 * 1024,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub socket_path: String,
    pub terminal: TerminalConfig,
    pub process: ProcessConfig,
    pub network: NetworkConfig,
    pub firewall: FirewallConfig,
    pub files: FilesConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            socket_path: DEFAULT_SOCKET_PATH.to_string(),
            terminal: TerminalConfig::default(),
            process: ProcessConfig::default(),
            network: NetworkConfig::default(),
            firewall: FirewallConfig::default(),
            files: FilesConfig::default(),
        }
    }
}

impl Config {
    /// Defaults that make a debug build convenient for local development.
    ///
    /// Production defaults stay safe: process control, network control, firewall
    /// control and file writes are off unless an existing config file explicitly
    /// enables them.
    pub fn dev_default() -> Self {
        let mut config = Self::default();
        config.process.control = true;
        config.network.control = true;
        config.firewall.control = true;
        config.files.write = true;
        config
    }

    /// Defaults used when a server subcommand has to create a missing config.
    pub fn server_default() -> Self {
        if cfg!(debug_assertions) {
            Self::dev_default()
        } else {
            Self::default()
        }
    }
}

impl Config {
    pub fn from_toml(text: &str) -> Result<Self, Box<dyn Error + Send + Sync>> {
        match toml::from_str(text) {
            Ok(config) => Ok(config),
            Err(e) => {
                eprintln!("Error parsing TOML: {e}");
                Err(Box::new(e).into())
            }
        }
    }

    pub fn to_toml(&self) -> Result<String, Box<dyn Error + Send + Sync>> {
        match toml::to_string_pretty(self) {
            Ok(body) => Ok(body),
            Err(e) => {
                eprintln!("Error serializing config to TOML: {e}");
                Err(Box::new(e).into())
            }
        }
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();

        match std::fs::read_to_string(path) {
            Ok(text) => Self::from_toml(&text)
                .map_err(|source| format!("Error parsing config at {}: {source}", path.display())),
            Err(source) => Err(format!(
                "Error reading config at {}: {source}",
                path.display()
            )),
        }
    }

    /// Loads the configuration at `path`, falling back to in-memory defaults when
    /// it cannot be read. Never touches the filesystem, so it is the right entry
    /// point for the client subcommands.
    pub fn load_or_default(path: impl AsRef<Path>) -> Self {
        Self::load(path).unwrap_or_else(|e| {
            eprintln!("Failed to load config: {e}");
            Self::default()
        })
    }

    /// Loads the configuration at `path`, or writes a default one first when it
    /// cannot be read, so a first server run bootstraps the configuration file.
    /// Debug builds bootstrap with process control, network control, firewall
    /// control and file writes enabled; release builds keep all four off. A failure to persist the default is
    /// reported but not fatal: the freshly built defaults are still usable.
    pub fn load_or_create(path: impl AsRef<Path>) -> Self {
        match Self::load(&path) {
            Ok(config) => config,
            Err(e) => {
                eprintln!("Failed to load config: {e}");
                eprintln!("Using default config.");

                let config = Self::server_default();
                if cfg!(debug_assertions) {
                    eprintln!(
                        "Debug build: process control, network control, firewall control and file writes are enabled in the generated config."
                    );
                }
                if let Err(e) = config.save(&path) {
                    eprintln!("Failed to save default config: {e}");
                }
                config
            }
        }
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), String> {
        let path = path.as_ref();

        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|source| {
                format!(
                    "Error creating config directory at {}: {source}",
                    parent.display()
                )
            })?;
        }

        std::fs::write(
            path,
            self.to_toml().map_err(|source| {
                format!("Error serializing config at {}: {source}", path.display())
            })?,
        )
        .map_err(|source| format!("Error writing config at {}: {source}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_shell_uses_auto() {
        assert_eq!(TerminalConfig::default().shell, "auto");
    }

    #[test]
    fn firewall_control_is_off_by_default_and_on_in_debug_defaults() {
        assert!(!FirewallConfig::default().control);
        if cfg!(debug_assertions) {
            assert!(Config::dev_default().firewall.control);
        }
    }

    #[test]
    fn firewall_section_is_loaded_from_toml() {
        let config = Config::from_toml(
            r#"
[firewall]
control = true
"#,
        )
        .unwrap();

        assert!(config.firewall.control);
    }

    #[test]
    fn file_defaults_are_safe() {
        let files = FilesConfig::default();
        assert_eq!(files.root, "/");
        assert!(!files.write);
        assert_eq!(files.max_entries, 5000);
        assert_eq!(files.max_preview_bytes, 1024 * 1024);
        assert_eq!(files.max_upload_bytes, 64 * 1024 * 1024);
    }

    #[test]
    fn network_control_is_off_by_default_and_on_in_debug_defaults() {
        assert!(!NetworkConfig::default().control);
        if cfg!(debug_assertions) {
            assert!(Config::dev_default().network.control);
        }
    }

    #[test]
    fn network_section_is_loaded_from_toml() {
        let config = Config::from_toml(
            r#"
[network]
control = true
"#,
        )
        .unwrap();

        assert!(config.network.control);
    }

    #[test]
    fn files_section_is_loaded_from_toml() {
        let config = Config::from_toml(
            r#"
[files]
root = "/srv"
write = true
max_entries = 10
max_preview_bytes = 20
max_upload_bytes = 30
"#,
        )
        .unwrap();

        assert_eq!(config.files.root, "/srv");
        assert!(config.files.write);
        assert_eq!(config.files.max_entries, 10);
        assert_eq!(config.files.max_preview_bytes, 20);
        assert_eq!(config.files.max_upload_bytes, 30);
    }

    #[test]
    fn an_explicit_shell_wins_over_the_environment() {
        assert_eq!(
            resolve_shell("/bin/bash", Some("/bin/zsh"), Some("/bin/sh")),
            "/bin/bash".to_string()
        );
    }

    #[test]
    fn auto_uses_an_existing_environment_shell() {
        assert_eq!(
            resolve_shell("auto", Some("/bin/sh"), Some("/bin/dash")),
            "/bin/sh".to_string()
        );
    }

    #[test]
    fn auto_falls_back_to_the_login_shell_then_bin_sh() {
        assert_eq!(
            resolve_shell("auto", Some("/nonexistent/shell"), Some("/bin/sh")),
            "/bin/sh".to_string()
        );
        assert_eq!(
            resolve_shell("auto", None, Some("/bin/sh")),
            "/bin/sh".to_string()
        );
        assert_eq!(resolve_shell("auto", None, None), "/bin/sh".to_string());
        assert_eq!(resolve_shell("", Some(""), None), "/bin/sh".to_string());
    }

    #[test]
    fn passwd_entries_are_matched_by_uid() {
        let passwd = concat!(
            "root:x:0:0:root:/root:/bin/sh\n",
            "alice:x:1000:1000:Alice:/home/alice:/bin/sh\n",
            "broken:x:1001:1001:Broken:/home/broken:\n",
        );

        assert_eq!(shell_from_passwd(passwd, 0), Some("/bin/sh".to_string()));
        assert_eq!(shell_from_passwd(passwd, 1000), Some("/bin/sh".to_string()));
        assert_eq!(shell_from_passwd(passwd, 1001), None);
        assert_eq!(shell_from_passwd(passwd, 9999), None);
    }
}
