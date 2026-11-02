//! Interface and app identifiers.
//!
//! An interface is `namespace:package/interface@major.minor.patch`; an app is a
//! reverse-DNS id. Both are parsed once and rendered canonically, because
//! comparing raw strings is how `@1.0` and `@1.0.0` become different things.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::Error;

/// A semantic version, as the interface id carries it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl Version {
    pub fn new(major: u64, minor: u64, patch: u64) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    pub fn parse(input: &str) -> Result<Self, Error> {
        let mut parts = input.split('.');
        let major = parts.next();
        let minor = parts.next();
        let patch = parts.next();
        let (Some(major), Some(minor), Some(patch), None) = (major, minor, patch, parts.next())
        else {
            return Err(Error::invalid(format!(
                "`{input}` is not a version of the form major.minor.patch"
            )));
        };
        Ok(Self::new(
            parse_number(major, input)?,
            parse_number(minor, input)?,
            parse_number(patch, input)?,
        ))
    }
}

fn parse_number(input: &str, whole: &str) -> Result<u64, Error> {
    if input.is_empty() || (input.len() > 1 && input.starts_with('0')) {
        return Err(Error::invalid(format!(
            "`{whole}` has a version component that is not a canonical number"
        )));
    }
    input.parse::<u64>().map_err(|_| {
        Error::invalid(format!(
            "`{whole}` has a version component that is not a number"
        ))
    })
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Who owns a namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Namespace {
    /// `anth`: the engine's own interfaces.
    Engine,
    /// `anthracite`: first-party apps.
    FirstParty,
    /// Anything else: third-party apps.
    ThirdParty,
}

impl Namespace {
    pub fn of(namespace: &str) -> Self {
        match namespace {
            "anth" => Self::Engine,
            "anthracite" => Self::FirstParty,
            _ => Self::ThirdParty,
        }
    }
}

/// `namespace:package/interface@major.minor.patch`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InterfaceId {
    namespace: String,
    package: String,
    interface: String,
    version: Version,
}

impl InterfaceId {
    pub fn new(
        namespace: &str,
        package: &str,
        interface: &str,
        version: Version,
    ) -> Result<Self, Error> {
        for (what, value) in [("namespace", namespace), ("package", package)] {
            if !is_kebab(value) {
                return Err(Error::invalid(format!(
                    "`{value}` is not a valid interface {what}"
                )));
            }
        }
        if !is_kebab(interface) {
            return Err(Error::invalid(format!(
                "`{interface}` is not a valid interface name"
            )));
        }
        Ok(Self {
            namespace: namespace.to_string(),
            package: package.to_string(),
            interface: interface.to_string(),
            version,
        })
    }

    pub fn parse(input: &str) -> Result<Self, Error> {
        let (head, version) = input
            .split_once('@')
            .ok_or_else(|| Error::invalid(format!("`{input}` has no `@version`")))?;
        let (namespace, rest) = head
            .split_once(':')
            .ok_or_else(|| Error::invalid(format!("`{input}` has no `namespace:`")))?;
        let (package, interface) = rest
            .split_once('/')
            .ok_or_else(|| Error::invalid(format!("`{input}` has no `package/interface`")))?;
        if interface.contains('/') {
            return Err(Error::invalid(format!(
                "`{input}` has more than one interface segment"
            )));
        }
        Self::new(namespace, package, interface, Version::parse(version)?)
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    pub fn package(&self) -> &str {
        &self.package
    }

    pub fn interface(&self) -> &str {
        &self.interface
    }

    pub fn version(&self) -> Version {
        self.version
    }

    pub fn namespace_kind(&self) -> Namespace {
        Namespace::of(&self.namespace)
    }

    /// `namespace:package`, the WIT package this interface belongs to.
    pub fn package_id(&self) -> String {
        format!("{}:{}", self.namespace, self.package)
    }
}

impl fmt::Display for InterfaceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}/{}@{}",
            self.namespace, self.package, self.interface, self.version
        )
    }
}

impl FromStr for InterfaceId {
    type Err = Error;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Self::parse(input)
    }
}

impl Serialize for InterfaceId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for InterfaceId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

/// A reverse-DNS app id: `org.example.notes`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AppId(String);

impl AppId {
    pub fn parse(input: &str) -> Result<Self, Error> {
        if input.len() > 255 {
            return Err(Error::invalid("an app id is longer than 255 characters"));
        }
        let labels: Vec<&str> = input.split('.').collect();
        if labels.len() < 2 {
            return Err(Error::invalid(format!(
                "`{input}` is not a reverse-DNS app id: it needs at least two labels"
            )));
        }
        for label in &labels {
            if !is_app_label(label) {
                return Err(Error::invalid(format!(
                    "`{input}` has an invalid label `{label}`"
                )));
            }
        }
        Ok(Self(input.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The last label, used as the CLI name when the manifest declares no alias.
    pub fn last_label(&self) -> &str {
        self.0.rsplit('.').next().unwrap_or(&self.0)
    }
}

impl fmt::Display for AppId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for AppId {
    type Err = Error;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Self::parse(input)
    }
}

impl Serialize for AppId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for AppId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

/// A WIT-ish identifier: lowercase, starts with a letter, `-` only between
/// alphanumeric runs.
pub fn is_kebab(input: &str) -> bool {
    let mut chars = input.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() {
        return false;
    }
    let mut previous_dash = false;
    for c in chars {
        match c {
            'a'..='z' | '0'..='9' => previous_dash = false,
            '-' if !previous_dash => previous_dash = true,
            _ => return false,
        }
    }
    !previous_dash
}

/// A DNS label: alphanumeric edges, hyphens inside.
pub fn is_app_label(input: &str) -> bool {
    if input.is_empty() || input.len() > 63 {
        return false;
    }
    let bytes = input.as_bytes();
    let ok_edge = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    if !ok_edge(bytes[0]) || !ok_edge(bytes[bytes.len() - 1]) {
        return false;
    }
    bytes
        .iter()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

/// A SHA-256 digest, lowercase hex.
pub fn is_sha256(input: &str) -> bool {
    input.len() == 64
        && input
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_interface_id_round_trips() {
        let id = InterfaceId::parse("anth:fs/list@1.2.3").expect("parse");
        assert_eq!(id.namespace(), "anth");
        assert_eq!(id.package(), "fs");
        assert_eq!(id.interface(), "list");
        assert_eq!(id.version(), Version::new(1, 2, 3));
        assert_eq!(id.to_string(), "anth:fs/list@1.2.3");
        assert_eq!(id.package_id(), "anth:fs");
        assert_eq!(id.namespace_kind(), Namespace::Engine);
    }

    #[test]
    fn the_namespace_decides_the_owner() {
        let first = InterfaceId::parse("anthracite:hello/greeting@1.0.0").unwrap();
        let third = InterfaceId::parse("example:notes/list@1.0.0").unwrap();
        assert_eq!(first.namespace_kind(), Namespace::FirstParty);
        assert_eq!(third.namespace_kind(), Namespace::ThirdParty);
    }

    #[test]
    fn malformed_interfaces_are_refused() {
        for input in [
            "anth:fs/list",
            "anth:fs/list@1.0",
            "anth:fs/list@1.0.0.0",
            "anth:fs/list@01.0.0",
            "Anth:fs/list@1.0.0",
            "anth:Fs/list@1.0.0",
            "anth:fs/-list@1.0.0",
            "anth:fs/list-@1.0.0",
            "anth:fs/li--st@1.0.0",
            "anth:fs/list/extra@1.0.0",
            "nocolon/list@1.0.0",
        ] {
            assert!(
                InterfaceId::parse(input).is_err(),
                "`{input}` should not parse"
            );
        }
    }

    #[test]
    fn an_app_id_round_trips() {
        let id = AppId::parse("org.example.notes").expect("parse");
        assert_eq!(id.as_str(), "org.example.notes");
        assert_eq!(id.last_label(), "notes");
    }

    #[test]
    fn malformed_app_ids_are_refused() {
        for input in [
            "notes",
            "Org.example",
            "org..notes",
            "org.example.",
            "org.-example",
            "org.exa_mple",
            "org.example.a-",
        ] {
            assert!(AppId::parse(input).is_err(), "`{input}` should not parse");
        }
    }

    #[test]
    fn digests_are_checked_for_shape() {
        assert!(is_sha256(&"a".repeat(64)));
        assert!(!is_sha256(&"a".repeat(63)));
        assert!(!is_sha256(&"A".repeat(64)));
        assert!(!is_sha256(&"g".repeat(64)));
    }
}
