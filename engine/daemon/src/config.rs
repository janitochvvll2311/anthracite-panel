//! Engine configuration: one TOML file, created on first boot (§6.7).
//!
//! Debug builds default to relative paths and permissive switches so a developer
//! does not need root; release builds default to absolute paths and closed
//! switches. Neither can be inferred from the other, so the profile is explicit.

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anth_contract::Error;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Release defaults, also written on first boot.
pub const TEMPLATE: &str = include_str!("../config.template.toml");

/// Which set of defaults a build uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    Release,
    Debug,
}

/// The whole file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub version: u32,
    pub socket: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub listen: Option<String>,
    pub state: PathBuf,
    #[serde(default)]
    pub backend: Backend,
    #[serde(default)]
    pub log: LogSection,
    #[serde(default)]
    pub auth: AuthSection,
    #[serde(default)]
    pub apps: AppsSection,
    #[serde(default)]
    pub limits: LimitsSection,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    #[default]
    Cranelift,
    /// Reserved for the hardened profile; not built in v0.5.0.
    Pulley,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogSection {
    #[serde(default)]
    pub level: LogLevel,
    #[serde(default)]
    pub format: LogFormat,
}

impl Default for LogSection {
    fn default() -> Self {
        Self {
            level: LogLevel::Info,
            format: LogFormat::Text,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Error,
    Warn,
    #[default]
    Info,
    Debug,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    #[default]
    Text,
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthSection {
    /// Sessions only apply to TCP; the socket uses peer credentials.
    #[serde(rename = "session-ttl", default = "default_session_ttl")]
    pub session_ttl: HumanDuration,
}

impl Default for AuthSection {
    fn default() -> Self {
        Self {
            session_ttl: default_session_ttl(),
        }
    }
}

fn default_session_ttl() -> HumanDuration {
    HumanDuration(Duration::from_secs(12 * 60 * 60))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppsSection {
    #[serde(rename = "require-signature", default = "yes")]
    pub require_signature: bool,
    #[serde(rename = "allow-unverified-assets", default)]
    pub allow_unverified_assets: bool,
    #[serde(rename = "max-bundle-bytes", default = "default_bundle_cap")]
    pub max_bundle_bytes: u64,
}

impl Default for AppsSection {
    fn default() -> Self {
        Self {
            require_signature: true,
            allow_unverified_assets: false,
            max_bundle_bytes: default_bundle_cap(),
        }
    }
}

fn yes() -> bool {
    true
}

fn default_bundle_cap() -> u64 {
    32 * 1024 * 1024
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LimitsSection {
    #[serde(default = "default_fuel")]
    pub fuel: u64,
    #[serde(rename = "memory-pages", default = "default_memory_pages")]
    pub memory_pages: u32,
    #[serde(rename = "wall-ms", default = "default_wall_ms")]
    pub wall_ms: u64,
}

impl Default for LimitsSection {
    fn default() -> Self {
        Self {
            fuel: default_fuel(),
            memory_pages: default_memory_pages(),
            wall_ms: default_wall_ms(),
        }
    }
}

fn default_fuel() -> u64 {
    10_000_000
}

fn default_memory_pages() -> u32 {
    256
}

fn default_wall_ms() -> u64 {
    5_000
}

impl Config {
    /// Paths for a release deployment.
    pub fn release() -> Self {
        toml::from_str(TEMPLATE).expect("the release template is valid")
    }

    /// Paths for a developer: relative, and permissive.
    pub fn debug() -> Self {
        let mut config = Self::release();
        config.socket = PathBuf::from("./anth.sock");
        config.state = PathBuf::from("./state");
        config.apps.require_signature = false;
        config.apps.allow_unverified_assets = true;
        config
    }

    pub fn defaults(profile: Profile) -> Self {
        match profile {
            Profile::Release => Self::release(),
            Profile::Debug => Self::debug(),
        }
    }

    pub fn parse(text: &str) -> Result<Self, Error> {
        toml::from_str(text).map_err(|error| {
            Error::invalid(format!("the configuration is not valid TOML: {error}"))
        })
    }

    pub fn load(path: &Path) -> Result<Self, Error> {
        let text = std::fs::read_to_string(path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                Error::not_found(format!("no configuration at {}", path.display()))
            } else {
                Error::internal(format!("reading {}: {error}", path.display()))
            }
        })?;
        Self::parse(&text)
    }

    /// Loads the file, writing it first with the profile's defaults if missing.
    pub fn load_or_create(path: &Path, profile: Profile) -> Result<Self, Error> {
        if path.exists() {
            return Self::load(path);
        }
        let config = Self::defaults(profile);
        let text = match profile {
            Profile::Release => TEMPLATE.to_string(),
            Profile::Debug => toml::to_string_pretty(&config)
                .map_err(|error| Error::internal(format!("rendering the config: {error}")))?,
        };
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).map_err(|error| {
                Error::internal(format!("creating {}: {error}", parent.display()))
            })?;
        }
        std::fs::write(path, text)
            .map_err(|error| Error::internal(format!("writing {}: {error}", path.display())))?;
        Ok(config)
    }

    /// Checks what a bad file can get wrong.
    pub fn validate(&self) -> Result<(), Error> {
        if self.version != 1 {
            return Err(Error::invalid(format!(
                "configuration version {} is not supported; this engine speaks 1",
                self.version
            )));
        }
        if self.socket.as_os_str().is_empty() {
            return Err(Error::invalid("`socket` cannot be empty"));
        }
        if self.state.as_os_str().is_empty() {
            return Err(Error::invalid("`state` cannot be empty"));
        }
        if self.apps.max_bundle_bytes == 0 {
            return Err(Error::invalid(
                "`apps.max-bundle-bytes` must be greater than zero",
            ));
        }
        if self.limits.fuel == 0 || self.limits.memory_pages == 0 || self.limits.wall_ms == 0 {
            return Err(Error::invalid(
                "every `limits` value must be greater than zero",
            ));
        }
        if self.auth.session_ttl.0.is_zero() {
            return Err(Error::invalid(
                "`auth.session-ttl` must be greater than zero",
            ));
        }
        Ok(())
    }
}

/// A duration written the way an operator writes it: `12h`, `30m`, `45s`, `2d`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HumanDuration(pub Duration);

impl fmt::Display for HumanDuration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let seconds = self.0.as_secs();
        if seconds.is_multiple_of(86_400) {
            write!(f, "{}d", seconds / 86_400)
        } else if seconds.is_multiple_of(3_600) {
            write!(f, "{}h", seconds / 3_600)
        } else if seconds.is_multiple_of(60) {
            write!(f, "{}m", seconds / 60)
        } else {
            write!(f, "{seconds}s")
        }
    }
}

impl Serialize for HumanDuration {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for HumanDuration {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        let (number, unit) = text.split_at(text.len().saturating_sub(1));
        let value: u64 = number
            .parse()
            .map_err(|_| serde::de::Error::custom(format!("`{text}` is not a duration")))?;
        let seconds = match unit {
            "s" => value,
            "m" => value * 60,
            "h" => value * 3_600,
            "d" => value * 86_400,
            _ => {
                return Err(serde::de::Error::custom(format!(
                    "`{text}` has no s, m, h or d suffix"
                )));
            }
        };
        Ok(Self(Duration::from_secs(seconds)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_template_matches_the_release_defaults() {
        let from_template = Config::parse(TEMPLATE).expect("template parses");
        assert_eq!(from_template, Config::release());
    }

    #[test]
    fn the_debug_defaults_are_relative_and_open() {
        let debug = Config::debug();
        assert_eq!(debug.socket, PathBuf::from("./anth.sock"));
        assert_eq!(debug.state, PathBuf::from("./state"));
        assert!(!debug.apps.require_signature);
        assert!(debug.apps.allow_unverified_assets);
    }

    #[test]
    fn unknown_keys_are_refused() {
        let text = TEMPLATE.replace("level = \"info\"", "level = \"info\"\nlvel = \"debug\"");
        let error = Config::parse(&text).expect_err("unknown key");
        assert!(error.message().contains("lvel"), "{error}");
    }

    #[test]
    fn a_zero_limit_is_refused() {
        let mut config = Config::release();
        config.limits.fuel = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn a_bad_version_is_refused() {
        let text = TEMPLATE.replace("version = 1", "version = 2");
        let config = Config::parse(&text).expect("parse");
        assert!(config.validate().is_err());
    }

    #[test]
    fn a_duration_round_trips() {
        for (text, seconds) in [
            ("45s", 45),
            ("30m", 1_800),
            ("12h", 43_200),
            ("2d", 172_800),
        ] {
            let parsed: HumanDuration = toml::from_str(&format!("d = \"{text}\""))
                .map(|t: Table| t.d)
                .expect("parse");
            assert_eq!(parsed.0, Duration::from_secs(seconds));
            assert_eq!(parsed.to_string(), text);
        }
    }

    #[test]
    fn a_duration_without_a_unit_is_refused() {
        assert!(toml::from_str::<Table>("d = \"12\"").is_err());
    }

    #[derive(Deserialize)]
    struct Table {
        d: HumanDuration,
    }

    #[test]
    fn a_missing_file_is_created() {
        let dir = std::env::temp_dir().join(format!("anth-config-{}", std::process::id()));
        let path = dir.join("config.toml");
        let _ = std::fs::remove_dir_all(&dir);
        let config = Config::load_or_create(&path, Profile::Release).expect("create");
        assert!(path.exists());
        assert_eq!(config, Config::release());
        let again = Config::load(&path).expect("load");
        assert_eq!(again, config);
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
}
