//! TLS material from the certificate store's published directory.
//!
//! The store publishes `{shared}/tls/<id>/fullchain.pem` and `privkey.pem`.
//! The proxy only ever reads ids; the private key never crosses WIT. A
//! partially written or unparseable file keeps the last good resolver and
//! surfaces the error in `status`.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::ring;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;
use rustls::{ClientConfig, DigitallySignedStruct, RootCertStore, ServerConfig, SignatureScheme};

use crate::config::{normalize_name, Config, Listener};

/// ALPN order matters: h2 first, then HTTP/1.1.
pub const ALPN: [&[u8]; 2] = [b"h2", b"http/1.1"];

/// Installs the ring provider process-wide. Called once from `main`.
pub fn install_provider() {
    let _ = ring::default_provider().install_default();
}

fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(ring::default_provider())
}

pub struct CertStore {
    dir: PathBuf,
}

impl CertStore {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Published certificate ids: a directory with both PEM files.
    pub fn ids(&self) -> Vec<String> {
        let mut ids = Vec::new();
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return ids;
        };
        for entry in entries.flatten() {
            if !entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if self.paths(&name).is_some() {
                ids.push(name);
            }
        }
        ids.sort();
        ids
    }

    fn paths(&self, id: &str) -> Option<(PathBuf, PathBuf)> {
        if !valid_id(id) {
            return None;
        }
        let chain = self.dir.join(id).join("fullchain.pem");
        let key = self.dir.join(id).join("privkey.pem");
        if chain.is_file() && key.is_file() {
            Some((chain, key))
        } else {
            None
        }
    }

    pub fn load(&self, id: &str) -> Result<Arc<CertifiedKey>, String> {
        let (chain_path, key_path) = self
            .paths(id)
            .ok_or_else(|| format!("certificate `{id}` is not published"))?;
        let mut reader = BufReader::new(
            File::open(&chain_path)
                .map_err(|error| format!("reading {}: {error}", chain_path.display()))?,
        );
        let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut reader)
            .collect::<Result<_, _>>()
            .map_err(|error| format!("parsing {}: {error}", chain_path.display()))?;
        if certs.is_empty() {
            return Err(format!("{} carries no certificate", chain_path.display()));
        }
        let mut reader = BufReader::new(
            File::open(&key_path)
                .map_err(|error| format!("reading {}: {error}", key_path.display()))?,
        );
        let key: PrivateKeyDer<'static> = rustls_pemfile::private_key(&mut reader)
            .map_err(|error| format!("parsing {}: {error}", key_path.display()))?
            .ok_or_else(|| format!("{} carries no private key", key_path.display()))?;
        let key = ring::sign::any_supported_type(&key)
            .map_err(|error| format!("{}: {error}", key_path.display()))?;
        Ok(Arc::new(CertifiedKey::new(certs, key)))
    }

    /// A cheap fingerprint of the published directory, so the watcher only
    /// rebuilds when a file actually changed.
    pub fn stamp(&self) -> Vec<(String, u64, u64)> {
        let mut stamp = Vec::new();
        for id in self.ids() {
            let (chain, key) = match self.paths(&id) {
                Some(paths) => paths,
                None => continue,
            };
            let modified = |path: &Path| {
                fs::metadata(path)
                    .and_then(|meta| meta.modified())
                    .ok()
                    .and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())
                    .map(|delta| delta.as_secs())
                    .unwrap_or(0)
            };
            stamp.push((id, modified(&chain), modified(&key)));
        }
        stamp
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
        && !id.starts_with('.')
}

/// SNI names map to the certificate their host names, with the listener's
/// default host as the fallback for clients that send no SNI or an unknown one.
#[derive(Default)]
pub struct SniResolver {
    by_name: HashMap<String, Arc<CertifiedKey>>,
    default: Option<Arc<CertifiedKey>>,
}

impl SniResolver {
    pub fn build(listener: &Listener, config: &Config, store: &CertStore) -> Result<Self, String> {
        let mut resolver = Self::default();
        for host in &config.hosts {
            let Some(id) = &host.certificate else {
                continue;
            };
            let key = store.load(id)?;
            for name in &host.names {
                if let Some(name) = normalize_name(name) {
                    resolver.by_name.insert(name, Arc::clone(&key));
                }
            }
        }
        if let Some(default_host) = &listener.default_host {
            if let Some(host) = config.host(default_host) {
                if let Some(id) = &host.certificate {
                    resolver.default = Some(store.load(id)?);
                }
            }
        }
        Ok(resolver)
    }

    /// The selection rule, exposed so a unit test does not need a ClientHello.
    /// Without a default host an unknown or absent SNI returns `None`, which
    /// fails the handshake: never a certificate that does not match.
    pub fn select(&self, server_name: Option<&str>) -> Option<Arc<CertifiedKey>> {
        server_name
            .map(|name| name.trim_end_matches('.').to_ascii_lowercase())
            .and_then(|name| self.by_name.get(&name).cloned())
            .or_else(|| self.default.clone())
    }
}

impl std::fmt::Debug for SniResolver {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SniResolver")
            .field("names", &self.by_name.keys().collect::<Vec<_>>())
            .field("default", &self.default.is_some())
            .finish()
    }
}

impl ResolvesServerCert for SniResolver {
    fn resolve(&self, hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        self.select(hello.server_name())
    }
}

pub fn server_config(
    listener: &Listener,
    config: &Config,
    store: &CertStore,
) -> Result<Arc<ServerConfig>, String> {
    let resolver = SniResolver::build(listener, config, store)?;
    let mut server = ServerConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .map_err(|error| format!("building the TLS policy: {error}"))?
        .with_no_client_auth()
        .with_cert_resolver(Arc::new(resolver));
    server.alpn_protocols = ALPN.iter().map(|protocol| protocol.to_vec()).collect();
    Ok(Arc::new(server))
}

/// The client policy for upstreams. `verify = false` is explicit per upstream
/// in the document and is shown in the panel.
pub fn client_config(verify: bool) -> Result<ClientConfig, String> {
    let builder = ClientConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .map_err(|error| format!("building the TLS policy: {error}"))?;
    let config = if verify {
        let mut roots = RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        builder.with_root_certificates(roots).with_no_client_auth()
    } else {
        builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(NoVerify))
            .with_no_client_auth()
    };
    // hyper-rustls sets ALPN itself from the enabled HTTP versions; pre-setting
    // it here trips its builder assertion.
    Ok(config)
}

/// `tls-verify = false` on one upstream only. Certificate and signature checks
/// are skipped for that upstream; the name stays in the document for the
/// operator to see.
#[derive(Debug)]
struct NoVerify;

impl ServerCertVerifier for NoVerify {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Host, Route};
    use rcgen::{generate_simple_self_signed, CertifiedKey as RcgenKey};

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("anth-proxy-tls-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn publish(dir: &Path, id: &str, name: &str) {
        let key = generate_simple_self_signed(vec![name.to_string()]).expect("certificate");
        let RcgenKey { cert, key_pair } = key;
        let target = dir.join(id);
        fs::create_dir_all(&target).expect("certificate dir");
        fs::write(target.join("fullchain.pem"), cert.pem()).expect("chain");
        fs::write(target.join("privkey.pem"), key_pair.serialize_pem()).expect("key");
    }

    fn host(id: &str, name: &str, certificate: Option<&str>) -> Host {
        Host {
            id: id.to_string(),
            names: vec![name.to_string()],
            certificate: certificate.map(str::to_string),
            acme: false,
            redirect_https: false,
            hsts: false,
            routes: vec![Route {
                id: "root".to_string(),
                r#match: "/".to_string(),
                upstream: Some("app".to_string()),
                strip_prefix: false,
                websocket: false,
                redirect: None,
                headers: Vec::new(),
                body_limit: None,
            }],
        }
    }

    #[test]
    fn a_published_certificate_is_listed_and_loaded() {
        let dir = temp_dir("list");
        publish(&dir, "site", "example.com");
        let store = CertStore::new(dir.clone());
        assert_eq!(store.ids(), vec!["site".to_string()]);
        let key = store.load("site").expect("loads");
        assert!(!key.cert.is_empty());

        assert!(store.load("missing").is_err());
        assert!(store.load("../site").is_err());
        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn sni_picks_the_named_certificate_or_the_default() {
        let dir = temp_dir("sni");
        publish(&dir, "one", "one.test");
        publish(&dir, "two", "two.test");
        let store = CertStore::new(dir.clone());
        let config = Config {
            version: 1,
            listeners: Vec::new(),
            upstreams: Vec::new(),
            hosts: vec![
                host("one", "one.test", Some("one")),
                host("two", "two.test", Some("two")),
            ],
        };
        let listener = Listener {
            id: "secure".to_string(),
            bind: ":8443".to_string(),
            tls: true,
            default_host: Some("two".to_string()),
            forwarded: false,
        };
        let resolver = SniResolver::build(&listener, &config, &store).expect("resolver");
        let one = resolver.select(Some("ONE.TEST.")).expect("named");
        let two = resolver.select(Some("unknown.test")).expect("default");
        assert!(!Arc::ptr_eq(&one, &two));
        let default = resolver.select(None).expect("no SNI default");
        assert!(Arc::ptr_eq(&two, &default));

        let strict = Listener {
            id: "strict".to_string(),
            bind: ":8443".to_string(),
            tls: true,
            default_host: None,
            forwarded: false,
        };
        let resolver = SniResolver::build(&strict, &config, &store).expect("resolver");
        assert!(resolver.select(Some("unknown.test")).is_none());
        assert!(resolver.select(None).is_none());
        assert!(resolver.select(Some("one.test")).is_some());
        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn an_invalid_certificate_is_an_error_not_a_panic() {
        let dir = temp_dir("invalid");
        let target = dir.join("broken");
        fs::create_dir_all(&target).expect("dir");
        fs::write(target.join("fullchain.pem"), "not a chain").expect("chain");
        fs::write(target.join("privkey.pem"), "not a key").expect("key");
        let store = CertStore::new(dir.clone());
        assert!(store.load("broken").is_err());
        fs::remove_dir_all(dir).expect("cleanup");
    }
}
