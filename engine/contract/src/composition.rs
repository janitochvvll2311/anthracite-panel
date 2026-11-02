//! The composition document (§8.1).
//!
//! It is the declarative answer to "what is composed": a version, one profile and
//! the apps it lists. There is no provider map — the interface id is the binding —
//! and v0.5.0 accepts exactly one profile.

use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::id::{is_kebab, parse_version, AppId};

pub const COMPOSITION_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Composition {
    pub version: u32,
    #[serde(default)]
    pub profile: Vec<Profile>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub id: String,
    #[serde(default)]
    pub apps: Vec<String>,
}

impl Profile {
    /// The app id without its optional `@version` pin.
    pub fn app_id(entry: &str) -> &str {
        entry.split_once('@').map(|(id, _)| id).unwrap_or(entry)
    }

    /// The pinned version, when the entry carries one.
    pub fn app_version(entry: &str) -> Option<&str> {
        entry.split_once('@').map(|(_, version)| version)
    }
}

impl Composition {
    /// The document written on first boot: one profile, no apps.
    pub fn empty() -> Self {
        Self {
            version: COMPOSITION_VERSION,
            profile: vec![Profile {
                id: "default".to_string(),
                apps: Vec::new(),
            }],
        }
    }

    #[cfg(feature = "parse")]
    pub fn parse(text: &str) -> Result<Self, Error> {
        toml::from_str(text)
            .map_err(|error| Error::invalid(format!("the composition is not valid TOML: {error}")))
    }

    /// Reads the composition, or returns the empty one when the file is missing.
    #[cfg(feature = "parse")]
    pub fn load(path: &std::path::Path) -> Result<Self, Error> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::empty()),
            Err(error) => Err(Error::internal(format!(
                "reading {}: {error}",
                path.display()
            ))),
        }
    }

    pub fn validate(&self) -> Result<(), Error> {
        if self.version != COMPOSITION_VERSION {
            return Err(Error::invalid(format!(
                "composition version {} is not supported; this engine speaks {COMPOSITION_VERSION}",
                self.version
            )));
        }
        if self.profile.len() != 1 {
            return Err(Error::invalid(format!(
                "v0.5.0 composes exactly one profile, and this document has {}",
                self.profile.len()
            )));
        }
        for profile in &self.profile {
            if !is_kebab(&profile.id) {
                return Err(Error::invalid(format!(
                    "`{}` is not a valid profile id",
                    profile.id
                )));
            }
            for entry in &profile.apps {
                let id = Profile::app_id(entry);
                AppId::parse(id)?;
                if let Some(version) = Profile::app_version(entry) {
                    parse_version(version)?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_empty_document_validates() {
        Composition::empty().validate().expect("empty");
    }

    #[test]
    fn a_document_round_trips_through_toml() {
        let text = "version = 1\n\n[[profile]]\nid = \"default\"\napps = [\"org.example.notes\", \"org.anth.fs@0.6.0\"]\n";
        let composition = Composition::parse(text).expect("parse");
        composition.validate().expect("validate");
        assert_eq!(composition.profile[0].apps.len(), 2);
        assert_eq!(Profile::app_id("org.anth.fs@0.6.0"), "org.anth.fs");
        assert_eq!(Profile::app_version("org.anth.fs@0.6.0"), Some("0.6.0"));
        assert_eq!(Profile::app_version("org.example.notes"), None);
    }

    #[test]
    fn a_second_profile_is_refused() {
        let text = "version = 1\n\n[[profile]]\nid = \"default\"\n\n[[profile]]\nid = \"other\"\n";
        let composition = Composition::parse(text).expect("parse");
        let error = composition.validate().expect_err("two profiles");
        assert!(error.message().contains("exactly one profile"), "{error}");
    }

    #[test]
    fn an_unknown_key_is_refused() {
        let text = "version = 1\n\n[[profile]]\nid = \"default\"\naps = []\n";
        assert!(Composition::parse(text).is_err());
    }

    #[test]
    fn a_bad_app_id_is_refused() {
        let text = "version = 1\n\n[[profile]]\nid = \"default\"\napps = [\"notes\"]\n";
        let composition = Composition::parse(text).expect("parse");
        assert!(composition.validate().is_err());
    }

    #[test]
    fn a_bad_pin_is_refused() {
        let text =
            "version = 1\n\n[[profile]]\nid = \"default\"\napps = [\"org.example.notes@1.0\"]\n";
        let composition = Composition::parse(text).expect("parse");
        assert!(composition.validate().is_err());
    }

    #[test]
    fn the_empty_document_is_what_state_writes() {
        let parsed = Composition::parse(EMPTY).expect("parse");
        assert_eq!(parsed, Composition::empty());
    }

    const EMPTY: &str = "version = 1\n\n[[profile]]\nid = \"default\"\napps = []\n";
}
