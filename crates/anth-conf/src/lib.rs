use serde::{Deserialize, Serialize};
use std::{error::Error, path::Path};

pub const DEFAULT_CONFIG_PATH: &str = "/etc/anth/config.toml";
pub const DEFAULT_SOCKET_PATH: &str = "/var/tmp/anth.sock";

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub socket_path: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            socket_path: DEFAULT_SOCKET_PATH.to_string(),
        }
    }
}

impl Config {
    pub fn from_toml(text: &str) -> Result<Self, Box<dyn Error>> {
        toml::from_str(text).map_err(|e| Box::new(e) as Box<dyn Error>)
    }

    pub fn to_toml(&self) -> Result<String, Box<dyn Error>> {
        let body = toml::to_string_pretty(self).map_err(|e| Box::new(e) as Box<dyn Error>)?;
        Ok(body)
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
