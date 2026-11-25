//! Service configuration: the CA directory allowlist and the DNS solvers.
//!
//! The file is `{data}/config.json` (0600) and is written with a useful default
//! the first time the service starts. It is the only place that names a CA or a
//! DNS provider, so a route can never point the client at an arbitrary URL.

use std::fs;
use std::io::Write as _;
use std::net::IpAddr;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use anth_contract::Error;
use serde::{Deserialize, Serialize};
use url::Url;

pub const DEFAULT_HTTP01_PORT: u16 = 80;
pub const DEFAULT_RENEW_SECS: u64 = 1_800;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Directory {
    pub id: String,
    pub label: String,
    pub url: String,
    #[serde(default)]
    pub staging: bool,
    #[serde(rename = "allow-private", default)]
    pub allow_private: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Solver {
    pub id: String,
    pub kind: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub server: Option<String>,
    #[serde(default)]
    pub zone: Option<String>,
    #[serde(rename = "key-name", default)]
    pub key_name: Option<String>,
    #[serde(default)]
    pub secret: Option<String>,
    #[serde(default)]
    pub algorithm: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(rename = "http-01-port", default = "default_http01_port")]
    pub http01_port: u16,
    #[serde(rename = "renew-interval-secs", default = "default_renew_secs")]
    pub renew_interval_secs: u64,
    #[serde(default = "default_directories")]
    pub directories: Vec<Directory>,
    #[serde(default = "default_solvers")]
    pub solvers: Vec<Solver>,
    /// Extra PEM roots for an internal CA (the opt-in side of `allow-private`).
    #[serde(rename = "ca-bundle", default)]
    pub ca_bundle: Option<String>,
}

fn default_http01_port() -> u16 {
    DEFAULT_HTTP01_PORT
}

fn default_renew_secs() -> u64 {
    DEFAULT_RENEW_SECS
}

fn default_directories() -> Vec<Directory> {
    vec![
        Directory {
            id: "letsencrypt-staging".to_string(),
            label: "Let's Encrypt staging".to_string(),
            url: "https://acme-staging-v02.api.letsencrypt.org/directory".to_string(),
            staging: true,
            allow_private: false,
        },
        Directory {
            id: "letsencrypt".to_string(),
            label: "Let's Encrypt".to_string(),
            url: "https://acme-v02.api.letsencrypt.org/directory".to_string(),
            staging: false,
            allow_private: false,
        },
    ]
}

fn default_solvers() -> Vec<Solver> {
    vec![Solver {
        id: "manual".to_string(),
        kind: "manual".to_string(),
        url: None,
        token: None,
        server: None,
        zone: None,
        key_name: None,
        secret: None,
        algorithm: None,
    }]
}

impl Default for Config {
    fn default() -> Self {
        Self {
            http01_port: DEFAULT_HTTP01_PORT,
            renew_interval_secs: DEFAULT_RENEW_SECS,
            directories: default_directories(),
            solvers: default_solvers(),
            ca_bundle: None,
        }
    }
}

impl Config {
    pub fn path(data: &Path) -> PathBuf {
        data.join("config.json")
    }

    /// Loads `{data}/config.json`, writing the default first when absent. The
    /// file is 0600 because a solver may carry a webhook token or a TSIG secret.
    pub fn load(data: &Path) -> Result<Self, Error> {
        let path = Self::path(data);
        if !path.exists() {
            let config = Config::default();
            let bytes = serde_json::to_vec_pretty(&config)
                .map_err(|error| Error::internal(format!("encoding the config: {error}")))?;
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
        if self.http01_port == 0 {
            return Err(Error::invalid("`http-01-port` must be 1-65535"));
        }
        if self.renew_interval_secs < 60 {
            return Err(Error::invalid("`renew-interval-secs` must be at least 60"));
        }
        let mut seen = std::collections::HashSet::new();
        for directory in &self.directories {
            if !seen.insert(directory.id.clone()) {
                return Err(Error::invalid(format!(
                    "directory `{}` is declared twice",
                    directory.id
                )));
            }
            let url = Url::parse(&directory.url).map_err(|error| {
                Error::invalid(format!("directory `{}`: {error}", directory.id))
            })?;
            if url.scheme() != "https" && !directory.allow_private {
                return Err(Error::invalid(format!(
                    "directory `{}` must be https; set `allow-private` only for an internal CA",
                    directory.id
                )));
            }
            if url.scheme() != "https" && url.scheme() != "http" {
                return Err(Error::invalid(format!(
                    "directory `{}` is not http or https",
                    directory.id
                )));
            }
            if !directory.allow_private {
                if let Some(host) = url.host_str() {
                    if is_private_host(host) {
                        return Err(Error::invalid(format!(
                            "directory `{}` points at a private address; set `allow-private` to opt in",
                            directory.id
                        )));
                    }
                }
            }
        }
        if let Some(path) = &self.ca_bundle {
            let pem = fs::read(path).map_err(|error| {
                Error::invalid(format!("reading the CA bundle {path}: {error}"))
            })?;
            if rustls_pemfile::certs(&mut pem.as_slice()).next().is_none() {
                return Err(Error::invalid(format!(
                    "the CA bundle {path} has no PEM certificate"
                )));
            }
        }
        let mut solvers = std::collections::HashSet::new();
        for solver in &self.solvers {
            if !solvers.insert(solver.id.clone()) {
                return Err(Error::invalid(format!(
                    "solver `{}` is declared twice",
                    solver.id
                )));
            }
            match solver.kind.as_str() {
                "manual" => {}
                "webhook" => {
                    let url = solver.url.as_deref().ok_or_else(|| {
                        Error::invalid(format!("solver `{}` needs a `url`", solver.id))
                    })?;
                    let parsed = Url::parse(url).map_err(|error| {
                        Error::invalid(format!("solver `{}`: {error}", solver.id))
                    })?;
                    if parsed.scheme() != "https" {
                        return Err(Error::invalid(format!(
                            "solver `{}` must use an https webhook",
                            solver.id
                        )));
                    }
                }
                "rfc2136" => {
                    for (field, value) in [
                        ("server", solver.server.as_deref()),
                        ("zone", solver.zone.as_deref()),
                        ("key-name", solver.key_name.as_deref()),
                        ("secret", solver.secret.as_deref()),
                    ] {
                        if value.unwrap_or_default().trim().is_empty() {
                            return Err(Error::invalid(format!(
                                "solver `{}` needs `{field}`",
                                solver.id
                            )));
                        }
                    }
                    if !solver.server.as_deref().unwrap_or_default().contains(':') {
                        return Err(Error::invalid(format!(
                            "solver `{}` needs `server` as host:port",
                            solver.id
                        )));
                    }
                    let algorithm = solver.algorithm.as_deref().unwrap_or("hmac-sha256");
                    if !matches!(algorithm, "hmac-sha256" | "hmac-sha512" | "hmac-sha1") {
                        return Err(Error::invalid(format!(
                            "solver `{}` has an unknown TSIG algorithm `{algorithm}`",
                            solver.id
                        )));
                    }
                }
                other => {
                    return Err(Error::invalid(format!(
                        "solver `{}` has an unknown kind `{other}`",
                        solver.id
                    )));
                }
            }
        }
        Ok(())
    }

    pub fn directory(&self, url: &str) -> Result<&Directory, Error> {
        self.directories
            .iter()
            .find(|directory| directory.url == url)
            .ok_or_else(|| {
                Error::denied(format!(
                    "the directory `{url}` is not in the configured allowlist"
                ))
            })
    }

    pub fn solver(&self, id: &str) -> Result<&Solver, Error> {
        self.solvers
            .iter()
            .find(|solver| solver.id == id)
            .ok_or_else(|| Error::not_found(format!("no solver called `{id}`")))
    }

    pub fn directory_urls(&self) -> Vec<String> {
        self.directories
            .iter()
            .map(|directory| directory.url.clone())
            .collect()
    }
}

/// Refuses loopback, link-local, unique-local, private and cloud-metadata
/// addresses. A hostname is not resolved here; the request path resolves it and
/// [`check_public_target`] repeats the test on every address.
pub fn is_private_host(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if host.eq_ignore_ascii_case("localhost") || host.ends_with(".localhost") {
        return true;
    }
    match host.parse::<IpAddr>() {
        Ok(address) => is_private_ip(address),
        Err(_) => false,
    }
}

pub fn is_private_ip(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => {
            address.is_loopback()
                || address.is_private()
                || address.is_link_local()
                || address.is_unspecified()
                || address.is_broadcast()
                || address.octets()[0] == 0
        }
        IpAddr::V6(address) => {
            address.is_loopback()
                || address.is_unspecified()
                || address.is_unique_local()
                || address.is_unicast_link_local()
                || address
                    .to_ipv4_mapped()
                    .map(is_private_ip_v4)
                    .unwrap_or(false)
        }
    }
}

fn is_private_ip_v4(address: std::net::Ipv4Addr) -> bool {
    address.is_loopback()
        || address.is_private()
        || address.is_link_local()
        || address.is_unspecified()
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

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("anthracite-panel").join(name);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("scratch");
        dir
    }

    #[test]
    fn the_default_config_is_written_once() {
        let dir = scratch("acme-config-default");
        let config = Config::load(&dir).expect("load");
        assert!(Config::path(&dir).exists());
        assert_eq!(config.directories.len(), 2);
        assert_eq!(config.solvers[0].kind, "manual");
        let mode = std::os::unix::fs::PermissionsExt::mode(
            &fs::metadata(Config::path(&dir))
                .expect("meta")
                .permissions(),
        );
        assert_eq!(mode & 0o777, 0o600);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unknown_directory_is_denied() {
        let config = Config::default();
        let error = config
            .directory("https://ca.example/directory")
            .expect_err("denied");
        assert!(error.to_string().contains("allowlist"));
    }

    #[test]
    fn a_private_directory_needs_the_explicit_opt_in() {
        let mut config = Config::default();
        config.directories.push(Directory {
            id: "internal".to_string(),
            label: "Internal".to_string(),
            url: "http://127.0.0.1:14000/dir".to_string(),
            staging: false,
            allow_private: false,
        });
        assert!(config.validate().is_err());
        config.directories.last_mut().expect("entry").allow_private = true;
        config.validate().expect("opt in");
    }

    #[test]
    fn private_addresses_are_recognized() {
        assert!(is_private_host("127.0.0.1"));
        assert!(is_private_host("10.0.0.5"));
        assert!(is_private_host("169.254.169.254"));
        assert!(is_private_host("localhost"));
        assert!(is_private_host("::1"));
        assert!(!is_private_host("acme-v02.api.letsencrypt.org"));
    }

    #[test]
    fn a_webhook_must_be_https() {
        let mut config = Config::default();
        config.solvers.push(Solver {
            id: "hook".to_string(),
            kind: "webhook".to_string(),
            url: Some("http://hooks.example/present".to_string()),
            token: None,
            server: None,
            zone: None,
            key_name: None,
            secret: None,
            algorithm: None,
        });
        assert!(config.validate().is_err());
        config.solvers.last_mut().expect("entry").url =
            Some("https://hooks.example/present".to_string());
        config.validate().expect("https");
    }

    #[test]
    fn an_rfc2136_solver_needs_its_five_fields() {
        let mut config = Config::default();
        config.solvers.push(Solver {
            id: "dns".to_string(),
            kind: "rfc2136".to_string(),
            url: None,
            token: None,
            server: Some("10.0.0.53:53".to_string()),
            zone: Some("example.com".to_string()),
            key_name: Some("acme".to_string()),
            secret: None,
            algorithm: None,
        });
        assert!(config.validate().is_err());
    }
}
