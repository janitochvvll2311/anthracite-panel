//! The live document: one immutable snapshot swapped after validation.
//!
//! A request never sees half an apply: it clones the current `Arc<Snapshot>`
//! and uses that config, plan, TLS resolver and upstream clients together. A
//! failed apply leaves the previous snapshot serving.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::io::Write as _;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anth_contract::Error;
use http::Uri;
use hyper_rustls::HttpsConnectorBuilder;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use rustls::ServerConfig;

use crate::config::{normalize_name, Config, Endpoints, Listener, Upstream};
use crate::logs::{AccessEvent, LogHub};
use crate::plane::ProxyBody;
use crate::tls::{self, CertStore};

pub type UpstreamClient = Client<hyper_rustls::HttpsConnector<HttpConnector>, ProxyBody>;

/// A listener as routing sees it.
#[derive(Debug, Clone)]
pub struct ListenerPlan {
    pub tls: bool,
    pub default_host: Option<String>,
    pub forwarded: bool,
}

/// A host with its names normalized once, not per request.
#[derive(Debug, Clone)]
pub struct HostPlan {
    pub id: String,
    pub names: Vec<String>,
    pub acme: bool,
    pub redirect_https: bool,
    pub hsts: bool,
    pub has_certificate: bool,
    pub routes: Vec<crate::config::Route>,
}

pub struct Plan {
    pub version: u64,
    pub listeners: HashMap<String, ListenerPlan>,
    pub hosts: Vec<HostPlan>,
    pub hosts_by_id: HashMap<String, usize>,
    pub https_port: Option<u16>,
}

impl Plan {
    pub fn build(config: &Config) -> Self {
        let mut listeners = HashMap::new();
        for listener in &config.listeners {
            listeners.insert(
                listener.bind.clone(),
                ListenerPlan {
                    tls: listener.tls,
                    default_host: listener.default_host.clone(),
                    forwarded: listener.forwarded,
                },
            );
        }
        let mut hosts = Vec::new();
        let mut hosts_by_id = HashMap::new();
        for host in &config.hosts {
            hosts_by_id.insert(host.id.clone(), hosts.len());
            hosts.push(HostPlan {
                id: host.id.clone(),
                names: host
                    .names
                    .iter()
                    .filter_map(|name| normalize_name(name))
                    .collect(),
                acme: host.acme,
                redirect_https: host.redirect_https,
                hsts: host.hsts,
                has_certificate: host.certificate.is_some(),
                routes: host.routes.clone(),
            });
        }
        let https_port = config
            .listeners
            .iter()
            .find(|listener| listener.tls)
            .and_then(|listener| bind_port(&listener.bind));
        Self {
            version: config.version,
            listeners,
            hosts,
            hosts_by_id,
            https_port,
        }
    }

    pub fn host(&self, id: &str) -> Option<&HostPlan> {
        self.hosts_by_id.get(id).map(|index| &self.hosts[*index])
    }

    /// Resolves the Host header (or the listener default) to one host.
    pub fn select_host(
        &self,
        bind: &str,
        host_header: Option<&str>,
    ) -> Option<(&ListenerPlan, &HostPlan, String)> {
        let listener = self.listeners.get(bind)?;
        let name = crate::config::normalize_host_header(host_header.unwrap_or_default());
        let matched = self
            .hosts
            .iter()
            .find(|host| host.names.iter().any(|candidate| candidate == &name))
            .or_else(|| {
                listener
                    .default_host
                    .as_deref()
                    .and_then(|id| self.host(id))
            })?;
        Some((listener, matched, name))
    }
}

pub struct Snapshot {
    pub config: Config,
    pub plan: Plan,
    pub tls: Option<Arc<ServerConfig>>,
    pub clients: HashMap<String, UpstreamClient>,
}

pub struct Status {
    pub running: bool,
    pub listeners: Vec<String>,
    pub last_reload: u64,
    pub last_error: Option<String>,
    pub version: u64,
}

/// Concurrent connections a listener keeps alive; further accepts wait in the
/// kernel backlog instead of spawning unbounded tasks.
pub const MAX_CONNECTIONS: usize = 1024;

pub struct Service {
    pub data: PathBuf,
    pub acme_root: PathBuf,
    pub broker: Option<PathBuf>,
    pub endpoints: Endpoints,
    pub store: CertStore,
    pub logs: LogHub,
    /// Serializes `apply` and certificate reloads: the version check, the
    /// persist and the swap are one critical section.
    write: Mutex<()>,
    /// One permit per live connection across both listeners.
    pub permits: Arc<tokio::sync::Semaphore>,
    snapshot: RwLock<Arc<Snapshot>>,
    status: Mutex<Status>,
    bound: RwLock<Vec<(String, String)>>,
    temp: AtomicU64,
    runtime: Option<tokio::runtime::Handle>,
}

impl Service {
    pub fn new(
        data: PathBuf,
        tls_dir: PathBuf,
        acme_root: PathBuf,
        broker: Option<PathBuf>,
        endpoints: Endpoints,
    ) -> Arc<Self> {
        let store = CertStore::new(tls_dir);
        let service = Arc::new(Self {
            data,
            acme_root,
            broker,
            endpoints,
            store,
            logs: LogHub::new(1024),
            write: Mutex::new(()),
            permits: Arc::new(tokio::sync::Semaphore::new(MAX_CONNECTIONS)),
            snapshot: RwLock::new(Arc::new(Snapshot {
                config: Config::default(),
                plan: Plan::build(&Config::default()),
                tls: None,
                clients: HashMap::new(),
            })),
            status: Mutex::new(Status {
                running: false,
                listeners: Vec::new(),
                last_reload: now(),
                last_error: None,
                version: 0,
            }),
            bound: RwLock::new(Vec::new()),
            temp: AtomicU64::new(0),
            runtime: tokio::runtime::Handle::try_current().ok(),
        });
        match service.load() {
            Ok(Some(snapshot)) => {
                let version = snapshot.config.version;
                *service.snapshot.write().expect("snapshot") = Arc::new(snapshot);
                let mut status = service.status.lock().expect("status");
                status.version = version;
                status.last_reload = now();
            }
            Ok(None) => {}
            Err(error) => service.note_error(&error),
        }
        service
    }

    pub fn snapshot(&self) -> Arc<Snapshot> {
        Arc::clone(&self.snapshot.read().expect("snapshot"))
    }

    pub fn status(&self) -> crate::admin::ProxyStatus {
        let status = self.status.lock().expect("status");
        let listeners: Vec<String> = self
            .bound
            .read()
            .expect("bound")
            .iter()
            .map(|(_, actual)| actual.clone())
            .collect();
        crate::admin::ProxyStatus {
            running: !listeners.is_empty(),
            listeners,
            last_reload: status.last_reload,
            last_error: status.last_error.clone(),
            version: status.version,
            dropped: self.logs.dropped(),
        }
    }

    /// The address a configured bind actually got (ephemeral ports in tests).
    pub fn bound(&self, bind: &str) -> Option<String> {
        self.bound
            .read()
            .expect("bound")
            .iter()
            .find(|(configured, _)| configured == bind)
            .map(|(_, actual)| actual.clone())
    }

    pub fn record(&self, event: AccessEvent) {
        self.logs.record(&event);
    }

    pub fn note_error(&self, error: &str) {
        let mut status = self.status.lock().expect("status");
        status.last_error = Some(error.to_string());
    }

    pub fn note_reload(&self, version: u64) {
        let mut status = self.status.lock().expect("status");
        status.version = version;
        status.last_reload = now();
        status.last_error = None;
    }

    pub fn record_bound(&self, bind: &str, address: String) {
        let mut bound = self.bound.write().expect("bound");
        bound.push((bind.to_string(), address));
        bound.sort();
    }

    /// Loads `{data}/config.json` and builds a snapshot that only serves, never
    /// binds. A broken file is reported in `status` and the proxy starts empty.
    fn load(&self) -> Result<Option<Snapshot>, String> {
        let path = self.data.join("config.json");
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("reading {}: {error}", path.display())),
        };
        let config: Config = serde_json::from_slice(&bytes)
            .map_err(|error| format!("parsing {}: {error}", path.display()))?;
        let snapshot = self
            .build(&config)
            .map_err(|error| error.message().to_string())?;
        Ok(Some(snapshot))
    }

    /// Validates and materializes a snapshot, including TLS and clients.
    fn build(&self, config: &Config) -> Result<Snapshot, Error> {
        let certificates: BTreeSet<String> = self.store.ids().into_iter().collect();
        config.validate(&self.endpoints, &certificates)?;
        let plan = Plan::build(config);
        let tls = match tls_listener(config) {
            Some(listener) => {
                let server = tls::server_config(listener, config, &self.store)
                    .map_err(|message| Error::invalid(format!("listeners: {message}")))?;
                Some(server)
            }
            None => None,
        };
        let mut clients = HashMap::new();
        for upstream in &config.upstreams {
            clients.insert(
                upstream.id.clone(),
                client_for(upstream)
                    .map_err(|message| Error::invalid(format!("upstreams: {message}")))?,
            );
        }
        Ok(Snapshot {
            config: config.clone(),
            plan,
            tls,
            clients,
        })
    }

    /// Compares the expected version, validates the whole document, persists it
    /// and swaps. Nothing is swapped when any step fails.
    pub fn apply(&self, config: Config, expected: u64) -> Result<Config, Error> {
        let _serial = self.write.lock().expect("proxy write");
        let current = self.snapshot();
        if expected != current.config.version {
            return Err(Error::conflict(format!(
                "the document is at version {}, not {expected}",
                current.config.version
            )));
        }
        let mut next = config;
        next.version = expected.saturating_add(1);
        let snapshot = match self.build(&next) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                self.note_error(error.message());
                return Err(error);
            }
        };
        if let Err(error) = self.persist(&next) {
            self.note_error(error.message());
            return Err(error);
        }
        *self.snapshot.write().expect("snapshot") = Arc::new(snapshot);
        self.note_reload(next.version);
        Ok(next)
    }

    /// Re-reads the published directory. An invalid file keeps the last good
    /// resolver and is reported; it never half-applies.
    pub fn reload_certificates(&self) -> Result<(), String> {
        let _serial = self.write.lock().expect("proxy write");
        let current = self.snapshot();
        let Some(listener) = tls_listener(&current.config) else {
            self.note_reload(current.config.version);
            return Ok(());
        };
        let server = match tls::server_config(listener, &current.config, &self.store) {
            Ok(server) => server,
            Err(message) => {
                self.note_error(&message);
                return Err(message);
            }
        };
        let snapshot = Snapshot {
            config: current.config.clone(),
            plan: Plan::build(&current.config),
            tls: Some(server),
            clients: current.clients.clone(),
        };
        // The clients are a map of pooled connections; cloning them keeps the
        // pools. The plan is cheap and rebuilt for clarity.
        *self.snapshot.write().expect("snapshot") = Arc::new(snapshot);
        self.note_reload(current.config.version);
        Ok(())
    }

    /// Watches the published directory on a fixed interval.
    pub fn watch_certificates(self: &Arc<Self>, interval: Duration) {
        let service = Arc::clone(self);
        tokio::spawn(async move {
            let mut stamp = service.store.stamp();
            loop {
                tokio::time::sleep(interval).await;
                let next = service.store.stamp();
                if next == stamp {
                    continue;
                }
                stamp = next;
                if let Err(error) = service.reload_certificates() {
                    service.note_error(&error);
                }
            }
        });
    }

    pub fn certificates(&self) -> Vec<String> {
        self.store.ids()
    }

    /// Sends one request to an upstream's origin and reports what came back.
    pub async fn test_upstream(&self, id: &str) -> Result<crate::admin::UpstreamCheck, Error> {
        let snapshot = self.snapshot();
        let upstream = snapshot
            .config
            .upstream(id)
            .ok_or_else(|| Error::not_found(format!("`{id}` is not a declared upstream")))?;
        let (scheme, authority) = crate::config::parse_upstream_url(&upstream.url)
            .map_err(|message| Error::invalid(format!("upstreams: {message}")))?;
        let uri: Uri = format!("{scheme}://{authority}/")
            .parse()
            .map_err(|_| Error::invalid("upstreams: the URL is not valid"))?;
        let client = snapshot
            .clients
            .get(id)
            .ok_or_else(|| Error::unavailable("the upstream client is missing"))?;
        let request = http::Request::builder()
            .method(http::Method::GET)
            .uri(uri)
            .header(http::header::HOST, authority.as_str())
            .body(crate::plane::empty())
            .map_err(|error| Error::internal(format!("building the probe: {error}")))?;
        let started = std::time::Instant::now();
        let result = tokio::time::timeout(
            Duration::from_millis(upstream.read_timeout_ms as u64),
            client.request(request),
        )
        .await;
        let latency_ms = started.elapsed().as_millis() as u64;
        match result {
            Ok(Ok(response)) => Ok(crate::admin::UpstreamCheck {
                id: id.to_string(),
                ok: response.status().is_success() || response.status().is_redirection(),
                status: Some(response.status().as_u16()),
                latency_ms,
                error: None,
            }),
            Ok(Err(error)) => Ok(crate::admin::UpstreamCheck {
                id: id.to_string(),
                ok: false,
                status: None,
                latency_ms,
                error: Some(error.to_string()),
            }),
            Err(_) => Ok(crate::admin::UpstreamCheck {
                id: id.to_string(),
                ok: false,
                status: None,
                latency_ms,
                error: Some("the upstream did not answer before the read timeout".to_string()),
            }),
        }
    }

    /// The sync side of `test-upstream`: `Handler::call` runs on the blocking
    /// pool, so it may block on the runtime it was built in.
    pub fn test_upstream_blocking(&self, id: &str) -> Result<crate::admin::UpstreamCheck, Error> {
        let Some(runtime) = self.runtime.as_ref() else {
            return Err(Error::unavailable("no runtime for the upstream probe"));
        };
        runtime.block_on(self.test_upstream(id))
    }

    fn persist(&self, config: &Config) -> Result<(), Error> {
        fs::create_dir_all(&self.data).map_err(|error| {
            Error::unavailable(format!("creating {}: {error}", self.data.display()))
        })?;
        let path = self.data.join("config.json");
        let sequence = self.temp.fetch_add(1, Ordering::Relaxed);
        let temporary = self
            .data
            .join(format!(".config.json.{}.{sequence}", std::process::id()));
        let bytes = serde_json::to_vec_pretty(config)
            .map_err(|error| Error::internal(format!("serializing the document: {error}")))?;
        {
            let mut file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .open(&temporary)
                .map_err(|error| {
                    Error::unavailable(format!("writing {}: {error}", temporary.display()))
                })?;
            file.write_all(&bytes)
                .and_then(|()| file.write_all(b"\n"))
                .and_then(|()| file.sync_all())
                .map_err(|error| {
                    Error::unavailable(format!("writing {}: {error}", temporary.display()))
                })?;
        }
        fs::rename(&temporary, &path).map_err(|error| {
            let _ = fs::remove_file(&temporary);
            Error::unavailable(format!("publishing {}: {error}", path.display()))
        })
    }
}

pub fn tls_listener(config: &Config) -> Option<&Listener> {
    config.listeners.iter().find(|listener| listener.tls)
}

fn client_for(upstream: &Upstream) -> Result<UpstreamClient, String> {
    let tls = tls::client_config(upstream.tls_verify)?;
    let mut http = HttpConnector::new();
    http.enforce_http(false);
    http.set_connect_timeout(Some(Duration::from_millis(
        upstream.connect_timeout_ms as u64,
    )));
    let https = HttpsConnectorBuilder::new()
        .with_tls_config(tls)
        .https_or_http()
        .enable_http1()
        .enable_http2()
        .wrap_connector(http);
    Ok(Client::builder(TokioExecutor::new()).build(https))
}

pub fn bind_port(bind: &str) -> Option<u16> {
    bind.rsplit(':').next()?.parse().ok()
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|delta| delta.as_secs())
        .unwrap_or(0)
}
