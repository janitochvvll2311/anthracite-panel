//! The installed app's descriptor: what the operator may change without
//! touching the bundle (§6.4, §6.8).
//!
//! The manifest is signed by the author, so the engine cannot write `enabled`
//! or `alias` into it. The grant freezes authority, so toggling an app must not
//! rewrite it. Those mutable, non-authority facts live here.

use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::id::{is_kebab, is_sha256, AppId};
use crate::manifest::RESERVED_ALIASES;

pub const DESCRIPTOR_VERSION: u32 = 1;

/// Which signer vouched for the bundle. v0.5.0 has no distribution key, so every
/// signed app is `third-party`; `system` is reserved for v0.6.0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tier {
    ThirdParty,
    System,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Descriptor {
    pub version: u32,
    pub app: AppId,
    /// The operator's switch. Installed apps start enabled.
    #[serde(default = "yes")]
    pub enabled: bool,
    /// The CLI alias the operator chose, when it differs from the manifest's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    /// The fingerprint that signed, or `None` for an accepted unsigned bundle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signer: Option<String>,
    pub tier: Tier,
    /// RFC 3339, UTC: when this version was installed.
    pub installed_at: String,
}

fn yes() -> bool {
    true
}

impl Descriptor {
    #[cfg(feature = "parse")]
    pub fn parse(text: &str) -> Result<Self, Error> {
        toml::from_str(text)
            .map_err(|error| Error::invalid(format!("the descriptor is not valid TOML: {error}")))
    }

    #[cfg(feature = "parse")]
    pub fn to_toml(&self) -> Result<String, Error> {
        toml::to_string_pretty(self)
            .map_err(|error| Error::internal(format!("rendering the descriptor: {error}")))
    }

    pub fn validate(&self) -> Result<(), Error> {
        if self.version != DESCRIPTOR_VERSION {
            return Err(Error::invalid(format!(
                "descriptor version {} is not supported; this engine speaks {DESCRIPTOR_VERSION}",
                self.version
            )));
        }
        if let Some(alias) = &self.alias {
            if !is_kebab(alias) {
                return Err(Error::invalid(format!(
                    "`{alias}` is not a valid CLI alias"
                )));
            }
            if RESERVED_ALIASES.contains(&alias.as_str()) {
                return Err(Error::invalid(format!("`{alias}` is a reserved command")));
            }
        }
        if let Some(signer) = &self.signer {
            if !is_sha256(signer) {
                return Err(Error::invalid(
                    "`signer` is not a fingerprint; it is 64 lowercase hex",
                ));
            }
        }
        humantime::parse_rfc3339(&self.installed_at)
            .map_err(|error| Error::invalid(format!("`installed_at` is not RFC 3339: {error}")))?;
        Ok(())
    }
}

#[cfg(all(test, feature = "parse"))]
mod tests {
    use super::*;

    fn sample() -> Descriptor {
        Descriptor {
            version: DESCRIPTOR_VERSION,
            app: AppId::parse("org.example.notes").expect("id"),
            enabled: true,
            alias: Some("notes".to_string()),
            signer: Some("a".repeat(64)),
            tier: Tier::ThirdParty,
            installed_at: "2026-11-03T20:00:00Z".to_string(),
        }
    }

    #[test]
    fn a_descriptor_validates() {
        sample().validate().expect("valid");
    }

    #[test]
    fn enabled_defaults_to_true() {
        let text = sample().to_toml().expect("render");
        let text = text.replace("enabled = true\n", "");
        assert!(Descriptor::parse(&text).expect("parse").enabled);
    }

    #[test]
    fn a_reserved_alias_is_refused() {
        let mut descriptor = sample();
        descriptor.alias = Some("list".to_string());
        assert!(descriptor.validate().is_err());
    }

    #[test]
    fn a_malformed_fingerprint_is_refused() {
        let mut descriptor = sample();
        descriptor.signer = Some("ABCD".to_string());
        assert!(descriptor.validate().is_err());
    }

    #[test]
    fn a_bad_timestamp_is_refused() {
        let mut descriptor = sample();
        descriptor.installed_at = "yesterday".to_string();
        assert!(descriptor.validate().is_err());
    }

    #[test]
    fn a_bad_version_is_refused() {
        let mut descriptor = sample();
        descriptor.version = 2;
        assert!(descriptor.validate().is_err());
    }

    #[test]
    fn an_unknown_key_is_refused() {
        let text = sample().to_toml().expect("render");
        let text = text.replace("enabled", "enable");
        assert!(Descriptor::parse(&text).is_err());
    }

    #[test]
    fn the_descriptor_round_trips() {
        let descriptor = sample();
        assert_eq!(
            Descriptor::parse(&descriptor.to_toml().expect("render")).expect("parse"),
            descriptor
        );
    }
}
