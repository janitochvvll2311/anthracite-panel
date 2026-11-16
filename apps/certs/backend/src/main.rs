//! Certificate inventory, custody and publication for the Certificates app.
//!
//! The service scans the roots named in its argv, owns the managed store under
//! `{data}/certs`, and publishes material atomically under `{shared}/tls`. The
//! private key leaves the process only through that directory; no function
//! returns it.

mod pem;

use std::collections::HashSet;
use std::fs;
use std::io::{Read, Write as _};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anth_contract::Error;
use anth_service::Handler;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use pem::{chain_ok, key_matches, parse_certs, parse_key, CertInfo, KeyPair};

const STORE: &str = "anthracite:certs/store@1.0.0";
const AUDIT: &str = "anth:audit/record@1.0.0";
const DAY: u64 = 86_400;
const EXPIRING_DAYS: u64 = 30;
static TEMP: AtomicU64 = AtomicU64::new(0);

type Found = (Vec<CertInfo>, Option<KeyPair>, Option<Meta>);

struct Service {
    data: PathBuf,
    publish: PathBuf,
    scans: Vec<PathBuf>,
    broker: Option<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Meta {
    source: String,
    #[serde(default)]
    auto_renew: bool,
}

impl Service {
    fn new(data: PathBuf, publish: PathBuf, scans: Vec<PathBuf>, broker: Option<PathBuf>) -> Self {
        Self {
            data,
            publish,
            scans,
            broker,
        }
    }

    /// Best-effort audit through the broker bridge. A missing bridge never
    /// changes the outcome of the operation; the private key is never in the
    /// detail.
    fn audit(&self, action: &str, detail: Value) {
        let Some(path) = self.broker.as_ref() else {
            return;
        };
        let Ok(body) = serde_json::to_vec(&json!({
            "interface": AUDIT,
            "function": "record",
            "input": {"action": action, "detail": detail},
        })) else {
            return;
        };
        let Ok(mut stream) = UnixStream::connect(path) else {
            return;
        };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
        let head = format!(
            "POST /__anth/call HTTP/1.1\r\nHost: anthracite\r\n\
             Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(&body);
        let _ = stream.flush();
        let mut answer = Vec::new();
        let _ = stream.read_to_end(&mut answer);
    }

    fn store(&self) -> PathBuf {
        self.data.join("certs")
    }

    fn inventory(&self) -> Result<Value, Error> {
        let mut records = Vec::new();
        let mut seen = HashSet::new();
        for id in self.managed_ids()? {
            match self.managed_record(&id) {
                Ok(Some((record, fingerprint))) => {
                    seen.insert(fingerprint);
                    records.push(record);
                }
                Ok(None) => {}
                Err(_) => records.push(self.corrupt_managed(&id)),
            }
        }
        for record in self.system_records(&seen)? {
            records.push(record);
        }
        records.sort_by(|left, right| {
            left["not-after"]
                .as_u64()
                .cmp(&right["not-after"].as_u64())
                .then_with(|| left["id"].as_str().cmp(&right["id"].as_str()))
        });
        Ok(Value::Array(records))
    }

    fn managed_ids(&self) -> Result<Vec<String>, Error> {
        let dir = self.store();
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(Error::unavailable(format!(
                    "reading {}: {error}",
                    dir.display()
                )))
            }
        };
        let mut ids = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| {
                Error::unavailable(format!("reading {}: {error}", dir.display()))
            })?;
            if entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
                ids.push(entry.file_name().to_string_lossy().to_string());
            }
        }
        ids.sort();
        Ok(ids)
    }

    fn managed_record(&self, id: &str) -> Result<Option<(Value, String)>, Error> {
        let dir = self.store().join(id);
        let fullchain = dir.join("fullchain.pem");
        let certs = match fs::read_to_string(&fullchain) {
            Ok(text) => parse_certs(&text)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(Error::unavailable(format!(
                    "reading {}: {error}",
                    fullchain.display()
                )))
            }
        };
        let Some(leaf) = certs.first() else {
            return Ok(None);
        };
        let meta = self.read_meta(&dir);
        let source = meta
            .as_ref()
            .map(|meta| meta.source.clone())
            .unwrap_or_else(|| "imported".to_string());
        let auto_renew = meta.map(|meta| meta.auto_renew).unwrap_or(false);
        Ok(Some((
            self.record(leaf, &source, auto_renew),
            leaf.fingerprint.clone(),
        )))
    }

    fn read_meta(&self, dir: &Path) -> Option<Meta> {
        let text = fs::read_to_string(dir.join("meta.json")).ok()?;
        serde_json::from_str(&text).ok()
    }

    fn system_records(&self, seen: &HashSet<String>) -> Result<Vec<Value>, Error> {
        let mut records = Vec::new();
        let mut known = seen.clone();
        for root in &self.scans {
            let readable = match fs::metadata(root) {
                Ok(metadata) if metadata.is_dir() => fs::read_dir(root).is_ok(),
                Ok(metadata) if metadata.is_file() => fs::read(root).is_ok(),
                _ => false,
            };
            if !readable {
                records.push(self.unreadable_root(root));
                continue;
            }
            let mut files = Vec::new();
            collect_cert_files(root, &mut files);
            for path in files {
                let Ok(text) = fs::read_to_string(&path) else {
                    continue;
                };
                let Ok(certs) = parse_certs(&text) else {
                    continue;
                };
                for cert in certs {
                    if !known.insert(cert.fingerprint.clone()) {
                        continue;
                    }
                    records.push(self.record(&cert, "system", false));
                }
            }
        }
        Ok(records)
    }

    fn corrupt_managed(&self, id: &str) -> Value {
        let record_id = if id.len() == 16 && id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            id.to_string()
        } else {
            short_hash(id)
        };
        json!({
            "id": record_id,
            "names": [id],
            "issuer": "corrupt managed store",
            "not-before": 0,
            "not-after": 0,
            "key-type": "unknown",
            "source": "imported",
            "fingerprint-sha256": "",
            "status": "invalid",
            "auto-renew": false,
        })
    }

    fn unreadable_root(&self, root: &Path) -> Value {
        json!({
            "id": short_hash(&root.display().to_string()),
            "names": [root.display().to_string()],
            "issuer": "unreadable scan root",
            "not-before": 0,
            "not-after": 0,
            "key-type": "unknown",
            "source": "system",
            "fingerprint-sha256": "",
            "status": "invalid",
            "auto-renew": false,
        })
    }

    fn record(&self, cert: &CertInfo, source: &str, auto_renew: bool) -> Value {
        json!({
            "id": &cert.fingerprint[..16],
            "names": cert.names,
            "issuer": cert.issuer,
            "not-before": cert.not_before,
            "not-after": cert.not_after,
            "key-type": cert.key_type,
            "source": source,
            "fingerprint-sha256": cert.fingerprint,
            "status": status(cert),
            "auto-renew": auto_renew,
        })
    }

    fn find(&self, id: &str) -> Result<Option<Found>, Error> {
        validate_id(id)?;
        let dir = self.store().join(id);
        let fullchain = dir.join("fullchain.pem");
        if let Ok(text) = fs::read_to_string(&fullchain) {
            let certs = parse_certs(&text)?;
            let key = fs::read_to_string(dir.join("privkey.pem"))
                .ok()
                .and_then(|text| parse_key(&text).ok());
            return Ok(Some((certs, key, self.read_meta(&dir))));
        }
        // A system certificate is inventory only: no key and no managed meta.
        let mut files = Vec::new();
        for root in &self.scans {
            collect_cert_files(root, &mut files);
        }
        for path in files {
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            let Ok(certs) = parse_certs(&text) else {
                continue;
            };
            if certs
                .first()
                .map(|cert| cert.fingerprint.starts_with(id))
                .unwrap_or(false)
            {
                return Ok(Some((certs, None, None)));
            }
        }
        Ok(None)
    }

    fn import(&self, input: &Value, source: &str) -> Result<Value, Error> {
        let cert_pem = text(input, "cert-pem")?;
        let chain_pem = input
            .get("chain-pem")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let key_pem = text(input, "key-pem")?;
        let leaf = parse_certs(cert_pem)?
            .into_iter()
            .next()
            .ok_or_else(|| Error::invalid("the certificate PEM contains no leaf"))?;
        let key = parse_key(key_pem)?;
        if !key_matches(&key, &leaf)? {
            return Err(Error::invalid(
                "the private key does not match the certificate",
            ));
        }
        let id = leaf.fingerprint[..16].to_string();
        let dir = self.store().join(&id);
        fs::create_dir_all(&dir)
            .map_err(|error| Error::unavailable(format!("creating {}: {error}", dir.display())))?;
        set_dir_mode(&dir)?;
        let fullchain = if chain_pem.trim().is_empty() {
            format!("{}\n", cert_pem.trim())
        } else {
            format!("{}\n{}\n", cert_pem.trim(), chain_pem.trim())
        };
        write_private(&dir.join("fullchain.pem"), fullchain.as_bytes())?;
        write_private(&dir.join("privkey.pem"), key_pem.as_bytes())?;
        let auto_renew = source == "acme"
            && input
                .get("auto-renew")
                .and_then(Value::as_bool)
                .unwrap_or(false);
        let meta = Meta {
            source: source.to_string(),
            auto_renew,
        };
        write_private(
            &dir.join("meta.json"),
            &serde_json::to_vec_pretty(&meta)
                .map_err(|error| Error::internal(format!("encoding meta: {error}")))?,
        )?;
        let record = self.record(&leaf, source, auto_renew);
        self.publish_id(&id, &leaf, fullchain.as_bytes())?;
        self.audit(
            "certs.import",
            json!({"id": id, "source": source, "names": leaf.names}),
        );
        Ok(record)
    }

    fn publish(&self, id: &str) -> Result<Value, Error> {
        validate_id(id)?;
        let store = self.store().join(id);
        let fullchain = fs::read(store.join("fullchain.pem")).map_err(|error| {
            Error::not_found(format!("reading the certificate `{id}`: {error}"))
        })?;
        let certs = parse_certs(&String::from_utf8_lossy(&fullchain))?;
        let leaf = certs
            .first()
            .ok_or_else(|| Error::invalid(format!("the certificate `{id}` has no leaf")))?;
        self.publish_id(id, leaf, &fullchain)?;
        self.audit("certs.publish", json!({"id": id}));
        let meta = self.read_meta(&store);
        let source = meta
            .as_ref()
            .map(|meta| meta.source.clone())
            .unwrap_or_else(|| "imported".to_string());
        let auto = meta.map(|meta| meta.auto_renew).unwrap_or(false);
        Ok(self.record(leaf, &source, auto))
    }

    fn publish_id(&self, id: &str, leaf: &CertInfo, fullchain: &[u8]) -> Result<(), Error> {
        let dir = self.publish.join(id);
        fs::create_dir_all(&dir)
            .map_err(|error| Error::unavailable(format!("creating {}: {error}", dir.display())))?;
        set_dir_mode(&dir)?;
        write_private(&dir.join("fullchain.pem"), fullchain)?;
        let key = fs::read(self.store().join(id).join("privkey.pem"))
            .map_err(|error| Error::not_found(format!("reading the key for `{id}`: {error}")))?;
        write_private(&dir.join("privkey.pem"), &key)?;
        let meta = self.read_meta(&self.store().join(id)).unwrap_or(Meta {
            source: "imported".to_string(),
            auto_renew: false,
        });
        write_private(
            &dir.join("meta.json"),
            &serde_json::to_vec_pretty(&meta)
                .map_err(|error| Error::internal(format!("encoding meta: {error}")))?,
        )?;
        let _ = leaf;
        Ok(())
    }

    fn remove(&self, id: &str) -> Result<bool, Error> {
        validate_id(id)?;
        let dir = self.store().join(id);
        if !dir.is_dir() {
            return Ok(false);
        }
        fs::remove_dir_all(&dir)
            .map_err(|error| Error::unavailable(format!("removing {}: {error}", dir.display())))?;
        self.audit("certs.remove", json!({"id": id}));
        // The published copy stays: the proxy keeps its last-good material until
        // it reloads, which is the point of the atomic publication.
        Ok(true)
    }

    fn set_auto_renew(&self, id: &str, enabled: bool) -> Result<Value, Error> {
        validate_id(id)?;
        let dir = self.store().join(id);
        let meta = self
            .read_meta(&dir)
            .ok_or_else(|| Error::not_found(format!("`{id}` is not a managed certificate")))?;
        if meta.source != "acme" {
            return Err(Error::denied(
                "only ACME-issued certificates can renew automatically",
            ));
        }
        let meta = Meta {
            source: meta.source,
            auto_renew: enabled,
        };
        let encoded = serde_json::to_vec_pretty(&meta)
            .map_err(|error| Error::internal(format!("encoding meta: {error}")))?;
        write_private(&dir.join("meta.json"), &encoded)?;
        let published = self.publish.join(id);
        if published.is_dir() {
            write_private(&published.join("meta.json"), &encoded)?;
        }
        self.audit("certs.auto-renew", json!({"id": id, "enabled": enabled}));
        let (certs, _, _) = self
            .find(id)?
            .ok_or_else(|| Error::not_found(format!("`{id}` is not installed")))?;
        let leaf = certs
            .first()
            .ok_or_else(|| Error::invalid(format!("`{id}` has no leaf")))?;
        Ok(self.record(leaf, "acme", enabled))
    }

    fn validate(&self, id: &str) -> Value {
        let Ok(Some((certs, key, meta))) = self.find(id) else {
            return json!({
                "ok": false,
                "reason": "not found",
                "chain-ok": false,
                "key-matches": false,
            });
        };
        let Some(leaf) = certs.first() else {
            return json!({
                "ok": false,
                "reason": "the certificate has no leaf",
                "chain-ok": false,
                "key-matches": false,
            });
        };
        let chain = chain_ok(&certs);
        let matches = match &key {
            Some(key) => key_matches(key, leaf).unwrap_or(false),
            None => false,
        };
        let current = status(leaf);
        let reason = if !chain {
            Some("the chain does not reach a self-signed root")
        } else if key.is_none() {
            Some("system certificate: the private key is not managed")
        } else if !matches {
            Some("the private key does not match the certificate")
        } else if current == "expired" {
            Some("the certificate has expired")
        } else if current == "invalid" {
            Some("the certificate is not valid yet")
        } else {
            None
        };
        let ok =
            chain && (key.is_none() || matches) && (current == "valid" || current == "expiring");
        let _ = meta;
        json!({
            "ok": ok,
            "reason": reason,
            "chain-ok": chain,
            "key-matches": matches,
        })
    }
}

impl Handler for Service {
    fn call(&self, interface: &str, function: &str, input: Value) -> Result<Value, Error> {
        if interface != STORE {
            return Err(Error::not_found(format!(
                "`{interface}` is not served by the certificate manager"
            )));
        }
        match function {
            "inventory" => self.inventory(),
            "get-certificate" => {
                let id = text(&input, "id")?;
                match self.find(id)? {
                    Some((certs, _, meta)) => {
                        let leaf = certs
                            .first()
                            .ok_or_else(|| Error::invalid("the certificate has no leaf"))?;
                        let source = meta
                            .as_ref()
                            .map(|meta| meta.source.clone())
                            .unwrap_or_else(|| "system".to_string());
                        let auto = meta.map(|meta| meta.auto_renew).unwrap_or(false);
                        Ok(self.record(leaf, &source, auto))
                    }
                    None => Ok(Value::Null),
                }
            }
            "import-certificate" => self.import(&input, "imported"),
            "store-issued" => self.import(&input, "acme"),
            "remove-certificate" => {
                let id = text(&input, "id")?;
                Ok(json!(self.remove(id)?))
            }
            "validate" => {
                let id = text(&input, "id")?;
                Ok(self.validate(id))
            }
            "publish" => {
                let id = text(&input, "id")?;
                self.publish(id)
            }
            "set-auto-renew" => {
                let id = text(&input, "id")?;
                let enabled = input
                    .get("enabled")
                    .and_then(Value::as_bool)
                    .ok_or_else(|| Error::invalid("the call needs an `enabled` flag"))?;
                self.set_auto_renew(id, enabled)
            }
            other => Err(Error::not_found(format!(
                "`{STORE}` has no function `{other}`"
            ))),
        }
    }
}

fn status(cert: &CertInfo) -> &'static str {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_secs())
        .unwrap_or(0);
    if now < cert.not_before {
        "invalid"
    } else if now >= cert.not_after {
        "expired"
    } else if cert.not_after - now <= EXPIRING_DAYS * DAY {
        "expiring"
    } else {
        "valid"
    }
}

fn text<'a>(input: &'a Value, key: &str) -> Result<&'a str, Error> {
    input
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::invalid(format!("the call needs `{key}`")))
}

/// Certificate ids are the 16-hex prefix of the SHA-256 fingerprint. Refusing
/// anything else keeps `Path::join` from accepting `..`, separators or dots.
fn validate_id(id: &str) -> Result<(), Error> {
    if id.len() == 16 && id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(Error::invalid(
            "the certificate id is not a 16-character hex fingerprint",
        ))
    }
}

fn short_hash(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn collect_cert_files(path: &Path, out: &mut Vec<PathBuf>) {
    let Ok(metadata) = fs::metadata(path) else {
        return;
    };
    if metadata.is_file() {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if name.ends_with(".pem") || name.ends_with(".crt") {
            out.push(path.to_path_buf());
        }
        return;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            for name in ["fullchain.pem", "cert.pem"] {
                let candidate = path.join(name);
                if candidate.is_file() {
                    out.push(candidate);
                }
            }
        } else {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if name.ends_with(".pem") || name.ends_with(".crt") {
                out.push(path);
            }
        }
    }
}

fn set_dir_mode(path: &Path) -> Result<(), Error> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|error| Error::unavailable(format!("setting {} to 0700: {error}", path.display())))
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let tmp = path.with_file_name(format!(
        ".{}.tmp-{}-{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("write"),
        std::process::id(),
        TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)
        .map_err(|error| Error::unavailable(format!("creating {}: {error}", tmp.display())))?;
    file.write_all(bytes)
        .map_err(|error| Error::unavailable(format!("writing {}: {error}", tmp.display())))?;
    file.sync_all()
        .map_err(|error| Error::unavailable(format!("syncing {}: {error}", tmp.display())))?;
    fs::rename(&tmp, path)
        .map_err(|error| Error::unavailable(format!("renaming {}: {error}", path.display())))?;
    Ok(())
}

fn argument(name: &str) -> Option<String> {
    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        if argument == name {
            return args.next();
        }
    }
    None
}

fn scans() -> Vec<PathBuf> {
    let mut values = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        if argument == "--scan" {
            if let Some(value) = args.next() {
                values.push(PathBuf::from(value));
            }
        }
    }
    if values.is_empty() {
        values.push(PathBuf::from("/etc/ssl/certs"));
    }
    values
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let socket = anth_service::socket_argument()?;
    let data = argument("--data")
        .map(PathBuf::from)
        .ok_or_else(|| Error::invalid("`--data` is required"))?;
    let shared = argument("--shared").map(PathBuf::from);
    let publish = argument("--publish")
        .map(PathBuf::from)
        .or_else(|| shared.map(|path| path.join("tls")))
        .ok_or_else(|| Error::invalid("`--publish` or `--shared` is required"))?;
    let broker = argument("--broker").map(PathBuf::from);
    let service = Arc::new(Service::new(data, publish, scans(), broker));
    anth_service::serve(&socket, service).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcgen::{date_time_ymd, CertificateParams, DnType, KeyPair as RcgenKey};

    fn service() -> (Service, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "anthracite-panel/certs-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let data = root.join("data");
        let shared = root.join("shared");
        fs::create_dir_all(&data).unwrap();
        fs::create_dir_all(&shared).unwrap();
        (
            Service::new(data, shared.join("tls"), Vec::new(), None),
            root,
        )
    }

    fn pair(name: &str) -> (String, String) {
        let mut params = CertificateParams::new(vec![name.to_string()]).unwrap();
        params
            .distinguished_name
            .push(DnType::CommonName, name.to_string());
        let key = RcgenKey::generate().unwrap();
        let cert = params.self_signed(&key).unwrap();
        (cert.pem(), key.serialize_pem())
    }

    fn import(service: &Service, cert: &str, chain: &str, key: &str) -> Result<Value, Error> {
        service.call(
            STORE,
            "import-certificate",
            json!({
                "cert-pem": cert,
                "chain-pem": chain,
                "key-pem": key,
                "names": [],
                "auto-renew": false,
            }),
        )
    }

    #[test]
    fn an_import_publishes_the_pair_with_0600() {
        let (service, root) = service();
        let (cert, key) = pair("example.test");
        let record = import(&service, &cert, "", &key).expect("import");
        let id = record["id"].as_str().unwrap();
        let stored = service.store().join(id);
        let published = service.publish.join(id);
        for path in [
            stored.join("fullchain.pem"),
            stored.join("privkey.pem"),
            published.join("fullchain.pem"),
            published.join("privkey.pem"),
            published.join("meta.json"),
        ] {
            assert!(path.is_file(), "{}", path.display());
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        assert_eq!(record["source"], "imported");
        assert_eq!(record["status"], "valid");
        assert_eq!(record["names"][0], "example.test");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_mismatched_key_is_refused_before_writing() {
        let (service, root) = service();
        let (cert, _) = pair("example.test");
        let (_, other) = pair("other.test");
        let error = import(&service, &cert, "", &other).expect_err("mismatch");
        assert!(error.message().contains("does not match"), "{error}");
        assert!(!service.store().exists() || fs::read_dir(service.store()).unwrap().count() == 0);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn an_expired_certificate_reports_expired() {
        let (service, root) = service();
        let mut params = CertificateParams::new(vec!["old.test".to_string()]).unwrap();
        params.not_before = date_time_ymd(2019, 1, 1);
        params.not_after = date_time_ymd(2020, 1, 1);
        let key = RcgenKey::generate().unwrap();
        let cert = params.self_signed(&key).unwrap();
        let record = import(&service, &cert.pem(), "", &key.serialize_pem()).expect("import old");
        assert_eq!(record["status"], "expired");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn removal_keeps_the_published_copy() {
        let (service, root) = service();
        let (cert, key) = pair("example.test");
        let record = import(&service, &cert, "", &key).expect("import");
        let id = record["id"].as_str().unwrap();
        assert!(service.remove(id).expect("remove"));
        assert!(!service.store().join(id).exists());
        assert!(service.publish.join(id).join("privkey.pem").is_file());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_certificate_without_its_root_fails_chain_validation() {
        let (service, root) = service();
        let mut ca_params = CertificateParams::new(vec!["ca.test".to_string()]).unwrap();
        ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        ca_params
            .distinguished_name
            .push(DnType::CommonName, "ca.test");
        let ca_key = RcgenKey::generate().unwrap();
        let ca = ca_params.self_signed(&ca_key).unwrap();
        let mut leaf_params = CertificateParams::new(vec!["leaf.test".to_string()]).unwrap();
        leaf_params
            .distinguished_name
            .push(DnType::CommonName, "leaf.test");
        let leaf_key = RcgenKey::generate().unwrap();
        let leaf = leaf_params.signed_by(&leaf_key, &ca, &ca_key).unwrap();
        let record = import(&service, &leaf.pem(), "", &leaf_key.serialize_pem()).expect("leaf");
        let id = record["id"].as_str().unwrap();
        let validation = service.validate(id);
        assert_eq!(validation["chain-ok"], false);
        assert_eq!(validation["ok"], false);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn only_acme_certificates_can_auto_renew() {
        let (service, root) = service();
        let (cert, key) = pair("example.test");
        let record = import(&service, &cert, "", &key).expect("import");
        let id = record["id"].as_str().unwrap();
        let error = service
            .call(STORE, "set-auto-renew", json!({"id": id, "enabled": true}))
            .expect_err("imported");
        assert!(error.message().contains("ACME"), "{error}");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_malformed_pem_is_a_typed_error() {
        let (service, root) = service();
        let error = import(&service, "not a pem", "", "not a key").expect_err("malformed");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Invalid);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_missing_scan_root_is_reported_not_a_failure() {
        let root = std::env::temp_dir().join(format!(
            "anthracite-panel/certs-scan-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let data = root.join("data");
        fs::create_dir_all(&data).unwrap();
        let service = Service::new(
            data,
            root.join("shared"),
            vec![root.join("does-not-exist")],
            None,
        );
        let inventory = service
            .call(STORE, "inventory", json!({}))
            .expect("inventory");
        let records = inventory.as_array().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["issuer"], "unreadable scan root");
        assert_eq!(records[0]["status"], "invalid");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_path_like_id_is_refused() {
        let (service, root) = service();
        for id in ["../etc", "a/b", "..", "0123456789abcde"] {
            let error = service
                .call(STORE, "remove-certificate", json!({"id": id}))
                .expect_err("id");
            assert_eq!(error.kind(), anth_contract::ErrorKind::Invalid, "{id}");
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn acme_auto_renew_updates_the_published_meta() {
        let (service, root) = service();
        let (cert, key) = pair("acme.test");
        let record = service
            .call(
                STORE,
                "store-issued",
                json!({
                    "cert-pem": cert,
                    "chain-pem": "",
                    "key-pem": key,
                    "names": [],
                    "auto-renew": true,
                }),
            )
            .expect("issue");
        let id = record["id"].as_str().unwrap();
        let published = service.publish.join(id).join("meta.json");
        let meta: serde_json::Value =
            serde_json::from_slice(&fs::read(&published).expect("meta")).expect("json");
        assert_eq!(meta["auto_renew"], true);
        service
            .call(STORE, "set-auto-renew", json!({"id": id, "enabled": false}))
            .expect("disable");
        let meta: serde_json::Value =
            serde_json::from_slice(&fs::read(&published).expect("meta")).expect("json");
        assert_eq!(meta["auto_renew"], false);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_corrupt_managed_entry_does_not_hide_the_inventory() {
        let (service, root) = service();
        let id = "0123456789abcdef";
        let dir = service.store().join(id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("fullchain.pem"), b"not a pem").unwrap();
        let inventory = service
            .call(STORE, "inventory", json!({}))
            .expect("inventory");
        let records = inventory.as_array().unwrap();
        assert!(
            records.iter().any(|record| {
                record["id"] == id
                    && record["issuer"] == "corrupt managed store"
                    && record["status"] == "invalid"
            }),
            "{inventory}"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_fullchain_ending_in_an_intermediate_is_valid() {
        let (service, root) = service();
        let mut ca_params = CertificateParams::new(vec!["root.test".to_string()]).unwrap();
        ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        ca_params
            .distinguished_name
            .push(DnType::CommonName, "root.test");
        let ca_key = RcgenKey::generate().unwrap();
        let ca = ca_params.self_signed(&ca_key).unwrap();
        let mut intermediate_params =
            CertificateParams::new(vec!["intermediate.test".to_string()]).unwrap();
        intermediate_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        intermediate_params
            .distinguished_name
            .push(DnType::CommonName, "intermediate.test");
        let intermediate_key = RcgenKey::generate().unwrap();
        let intermediate = intermediate_params
            .signed_by(&intermediate_key, &ca, &ca_key)
            .unwrap();
        let mut leaf_params = CertificateParams::new(vec!["leaf.test".to_string()]).unwrap();
        leaf_params
            .distinguished_name
            .push(DnType::CommonName, "leaf.test");
        let leaf_key = RcgenKey::generate().unwrap();
        let leaf = leaf_params
            .signed_by(&leaf_key, &intermediate, &intermediate_key)
            .unwrap();
        let record = import(
            &service,
            &leaf.pem(),
            &intermediate.pem(),
            &leaf_key.serialize_pem(),
        )
        .expect("fullchain");
        let id = record["id"].as_str().unwrap();
        let validation = service.validate(id);
        assert_eq!(validation["chain-ok"], true, "{validation}");
        assert_eq!(validation["ok"], true, "{validation}");
        let _ = fs::remove_dir_all(root);
    }
}
