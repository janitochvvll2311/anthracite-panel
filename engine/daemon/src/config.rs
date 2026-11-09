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

impl Default for Profile {
    /// A debug build defaults to open so a developer does not need root; a
    /// release build defaults to closed. The flags override it either way.
    fn default() -> Self {
        if cfg!(debug_assertions) {
            Self::Debug
        } else {
            Self::Release
        }
    }
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
    pub services: ServicesSection,
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

pub use anth_runtime::{LogFormat, LogLevel};

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

/// The startup decision on native service bundles (D-Service-Third-Party).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServicesSection {
    /// A third-party bundle that declares `[service]` is refused at install
    /// unless this is on. System-tier apps are unaffected.
    #[serde(rename = "allow-third-party", default)]
    pub allow_third_party: bool,
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

    /// Checks what a bad file can get wrong. A release build refuses the
    /// permissive switches even when the file asks for them: a debug build may
    /// be open, a shipped one may not.
    pub fn validate(&self, profile: Profile) -> Result<(), Error> {
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
        if let Some(address) = &self.listen {
            address
                .parse::<std::net::SocketAddr>()
                .map_err(|_| Error::invalid(format!("`{address}` is not a socket address")))?;
        }
        if profile == Profile::Release {
            if !self.socket.is_absolute() {
                return Err(Error::invalid(
                    "a release build needs an absolute `socket` path",
                ));
            }
            if !self.state.is_absolute() {
                return Err(Error::invalid(
                    "a release build needs an absolute `state` path",
                ));
            }
            if !self.apps.require_signature {
                return Err(Error::invalid(
                    "`apps.require-signature` cannot be false in a release build",
                ));
            }
            if self.apps.allow_unverified_assets {
                return Err(Error::invalid(
                    "`apps.allow-unverified-assets` is a development switch",
                ));
            }
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
        // The last character is the unit; slicing at a byte index found by
        // char_indices cannot split a multi-byte character.
        let mut chars = text.char_indices();
        let Some((index, unit)) = chars.next_back() else {
            return Err(serde::de::Error::custom("a duration cannot be empty"));
        };
        let number = &text[..index];
        let value: u64 = number
            .parse()
            .map_err(|_| serde::de::Error::custom(format!("`{text}` is not a duration")))?;
        let factor = match unit {
            's' => 1,
            'm' => 60,
            'h' => 3_600,
            'd' => 86_400,
            _ => {
                return Err(serde::de::Error::custom(format!(
                    "`{text}` has no s, m, h or d suffix"
                )));
            }
        };
        let seconds = value.checked_mul(factor).ok_or_else(|| {
            serde::de::Error::custom(format!("`{text}` is too large to be a duration"))
        })?;
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
    fn third_party_services_are_off_by_default() {
        assert!(!Config::release().services.allow_third_party);
        assert!(!Config::debug().services.allow_third_party);
        let text = TEMPLATE.replace("allow-third-party = false", "allow-third-party = true");
        let config = Config::parse(&text).expect("parse");
        assert!(config.services.allow_third_party);
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
        assert!(config.validate(Profile::Release).is_err());
    }

    #[test]
    fn a_bad_version_is_refused() {
        let text = TEMPLATE.replace("version = 1", "version = 2");
        let config = Config::parse(&text).expect("parse");
        assert!(config.validate(Profile::Release).is_err());
    }

    #[test]
    fn a_release_build_refuses_the_permissive_switches() {
        let mut config = Config::release();
        config.apps.require_signature = false;
        assert!(config.validate(Profile::Release).is_err());
        assert!(config.validate(Profile::Debug).is_ok());

        let mut relative = Config::release();
        relative.state = PathBuf::from("./state");
        assert!(relative.validate(Profile::Release).is_err());
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

    #[test]
    fn a_duration_never_panics_on_bad_input() {
        for text in [
            "\"12é\"",
            "\"\"",
            "\"é\"",
            "\"99999999999999999999d\"",
            "\"18446744073709551615d\"",
        ] {
            let source = format!("d = {text}");
            assert!(
                toml::from_str::<Table>(&source).is_err(),
                "{source} should be refused"
            );
        }
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
