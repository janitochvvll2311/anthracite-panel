//! Store sources: the operator's catalogue allowlist.
//!
//! `{data}/sources.json` (0600) names every source and its Ed25519 public key.
//! An index cannot introduce its own trust root, and a URL cannot point the
//! service at an arbitrary local address unless the source opts in.

use std::fs;
use std::io::Write as _;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use anth_contract::Error;
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub url: String,
    #[serde(rename = "public-key")]
    pub public_key: String,
    #[serde(rename = "allow-private", default)]
    pub allow_private: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub sources: Vec<Source>,
}

impl Config {
    pub fn path(data: &Path) -> PathBuf {
        data.join("sources.json")
    }

    pub fn load(data: &Path) -> Result<Self, Error> {
        let path = Self::path(data);
        if !path.exists() {
            let bytes = serde_json::to_vec_pretty(&Config::default())
                .map_err(|error| Error::internal(format!("encoding the sources: {error}")))?;
            write_private(&path, &bytes)?;
        }
        let raw = fs::read(&path)
            .map_err(|error| Error::unavailable(format!("reading {}: {error}", path.display())))?;
        let config: Config = serde_json::from_slice(&raw)
            .map_err(|error| Error::invalid(format!("{}: {error}", path.display())))?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), Error> {
        let mut seen = std::collections::HashSet::new();
        for source in &self.sources {
            if !seen.insert(source.id.clone()) {
                return Err(Error::invalid(format!(
                    "source `{}` is declared twice",
                    source.id
                )));
            }
            if source.id.is_empty()
                || !source
                    .id
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || ".-".contains(character))
            {
                return Err(Error::invalid(format!(
                    "`{}` is not a source id",
                    source.id
                )));
            }
            let key = source.public_key.trim().to_ascii_lowercase();
            if key.len() != 64 || !key.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(Error::invalid(format!(
                    "source `{}` needs a 32-byte Ed25519 public key in hex",
                    source.id
                )));
            }
            match source.kind.as_str() {
                "native" | "github" => {
                    let url = Url::parse(&source.url).map_err(|error| {
                        Error::invalid(format!("source `{}`: {error}", source.id))
                    })?;
                    if !matches!(url.scheme(), "https" | "http") {
                        return Err(Error::invalid(format!(
                            "source `{}` is not an http(s) URL",
                            source.id
                        )));
                    }
                    if url.scheme() != "https" && !source.allow_private {
                        return Err(Error::invalid(format!(
                            "source `{}` must be https; allow-private is for a loopback fixture",
                            source.id
                        )));
                    }
                    if !source.allow_private {
                        if let Some(host) = url.host_str() {
                            if is_private_host(host) {
                                return Err(Error::denied(format!(
                                    "source `{}` points at a private address; set allow-private",
                                    source.id
                                )));
                            }
                        }
                    }
                }
                "local" => {
                    let name = source.url.trim();
                    if name.is_empty()
                        || name.starts_with('/')
                        || name
                            .split('/')
                            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
                    {
                        return Err(Error::invalid(format!(
                            "source `{}` is not a directory under the local root",
                            source.id
                        )));
                    }
                }
                other => {
                    return Err(Error::invalid(format!(
                        "source `{}` has an unknown kind `{other}`",
                        source.id
                    )));
                }
            }
        }
        Ok(())
    }

    pub fn source(&self, id: &str) -> Result<&Source, Error> {
        self.sources
            .iter()
            .find(|source| source.id == id)
            .ok_or_else(|| Error::not_found(format!("no source called `{id}`")))
    }

    pub fn normalized_key(&self, source: &Source) -> String {
        source.public_key.trim().to_ascii_lowercase()
    }
}

pub fn is_private_host(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if host.eq_ignore_ascii_case("localhost") || host.ends_with(".localhost") {
        return true;
    }
    match host.parse::<std::net::IpAddr>() {
        Ok(address) => is_private_ip(address),
        Err(_) => false,
    }
}

pub fn is_private_ip(address: std::net::IpAddr) -> bool {
    // Normalize an IPv4-mapped IPv6 address before classifying it: std treats
    // ::ffff:127.0.0.1 as neither loopback nor unique-local.
    let address = match address {
        std::net::IpAddr::V6(address) => address
            .to_ipv4_mapped()
            .map(std::net::IpAddr::V4)
            .unwrap_or(std::net::IpAddr::V6(address)),
        other => other,
    };
    match address {
        std::net::IpAddr::V4(address) => {
            let octets = address.octets();
            address.is_loopback()
                || address.is_private()
                || address.is_link_local()
                || address.is_unspecified()
                || address.is_broadcast()
                || (octets[0] == 100 && (64..128).contains(&octets[1]))
        }
        std::net::IpAddr::V6(address) => {
            address.is_loopback()
                || address.is_unspecified()
                || address.is_unique_local()
                || address.is_unicast_link_local()
        }
    }
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            Error::unavailable(format!("creating {}: {error}", parent.display()))
        })?;
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| Error::unavailable(format!("opening {}: {error}", path.display())))?;
    file.write_all(bytes)
        .map_err(|error| Error::unavailable(format!("writing {}: {error}", path.display())))?;
    file.sync_all()
        .map_err(|error| Error::unavailable(format!("syncing {}: {error}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(kind: &str, url: &str, private: bool) -> Source {
        Source {
            id: "one".to_string(),
            name: "One".to_string(),
            kind: kind.to_string(),
            url: url.to_string(),
            public_key: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                .to_string(),
            allow_private: private,
        }
    }

    #[test]
    fn a_private_native_source_needs_the_opt_in() {
        let config = Config {
            sources: vec![source("native", "http://127.0.0.1:9000", false)],
        };
        assert!(config.validate().is_err());
        let config = Config {
            sources: vec![source("native", "http://127.0.0.1:9000", true)],
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn a_local_source_cannot_escape_the_root() {
        for url in ["../store", "/etc", "a//b", ""] {
            let config = Config {
                sources: vec![source("local", url, false)],
            };
            assert!(config.validate().is_err(), "{url}");
        }
        let config = Config {
            sources: vec![source("local", "airgap", false)],
        };
        assert!(config.validate().is_ok());
    }
}
