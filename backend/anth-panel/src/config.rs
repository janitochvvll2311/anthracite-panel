use serde::{Deserialize, Serialize};
use std::{error::Error, path::Path};

pub const DEFAULT_CONFIG_PATH: &str = "/etc/anth/config.toml";
pub const DEFAULT_SOCKET_PATH: &str = "/var/tmp/anth.sock";

/// Web terminal settings. Off unless a token is configured.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct TerminalConfig {
    /// Program to spawn for each session.
    pub shell: String,
    /// Token required to open a session. Empty means the terminal is disabled.
    pub token: String,
    /// Maximum number of concurrent sessions across the daemon.
    pub max_sessions: usize,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            shell: "/bin/sh".to_string(),
            // Disabled by default: a terminal is remote code execution, so it
            // must be turned on deliberately rather than inherited by accident.
            token: String::new(),
            max_sessions: 4,
        }
    }
}

impl TerminalConfig {
    pub fn enabled(&self) -> bool {
        !self.token.is_empty()
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub socket_path: String,
    pub terminal: TerminalConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            socket_path: DEFAULT_SOCKET_PATH.to_string(),
            terminal: TerminalConfig::default(),
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
    /// A failure to persist the default is reported but not fatal: the freshly
    /// built defaults are still usable for this run.
    pub fn load_or_create(path: impl AsRef<Path>) -> Self {
        match Self::load(&path) {
            Ok(config) => config,
            Err(e) => {
                eprintln!("Failed to load config: {e}");
                eprintln!("Using default config.");

                let config = Self::default();
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
