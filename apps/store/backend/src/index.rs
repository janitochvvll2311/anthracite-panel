//! The signed catalogue format: exact-bytes verification and the WIT views.

use std::collections::HashSet;

use anth_contract::Error;
use base64::Engine as _;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub const MAX_BUNDLE: u64 = 32 * 1024 * 1024;
pub const MAX_INDEX: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Index {
    pub schema: u32,
    pub store: StoreInfo,
    pub apps: Vec<IndexApp>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StoreInfo {
    pub id: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IndexApp {
    pub id: String,
    pub name: String,
    pub summary: String,
    #[serde(default)]
    pub description: String,
    pub publisher: String,
    #[serde(default)]
    pub license: String,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    pub versions: Vec<IndexVersion>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IndexVersion {
    pub version: String,
    pub channel: String,
    #[serde(default)]
    pub published_at: u64,
    pub sha256: String,
    pub size: u64,
    pub bundle_url: String,
    pub signer: String,
    #[serde(default)]
    pub yanked: bool,
}

impl Index {
    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() > MAX_INDEX {
            return Err(Error::too_large("the index is larger than 8 MiB"));
        }
        let index: Index = serde_json::from_slice(bytes)
            .map_err(|error| Error::invalid(format!("the index is not JSON: {error}")))?;
        index.validate()?;
        Ok(index)
    }

    pub fn validate(&self) -> Result<(), Error> {
        if self.schema != 1 {
            return Err(Error::invalid(format!(
                "schema {} is not supported; the store speaks schema 1",
                self.schema
            )));
        }
        if self.store.id.trim().is_empty() {
            return Err(Error::invalid("the index has no store id"));
        }
        let mut apps = HashSet::new();
        for app in &self.apps {
            if !apps.insert(app.id.clone()) {
                return Err(Error::invalid(format!("app `{}` is listed twice", app.id)));
            }
            if !valid_id(&app.id) {
                return Err(Error::invalid(format!("`{}` is not an app id", app.id)));
            }
            if app.versions.is_empty() {
                return Err(Error::invalid(format!("`{}` has no version", app.id)));
            }
            let mut versions = HashSet::new();
            for version in &app.versions {
                if !versions.insert(version.version.clone()) {
                    return Err(Error::invalid(format!(
                        "`{}` lists `{}` twice",
                        app.id, version.version
                    )));
                }
                if !matches!(version.channel.as_str(), "stable" | "beta" | "nightly") {
                    return Err(Error::invalid(format!(
                        "`{}` uses an unknown channel `{}`",
                        app.id, version.channel
                    )));
                }
                if !hex64(&version.sha256) {
                    return Err(Error::invalid(format!(
                        "`{}` has an invalid sha256",
                        app.id
                    )));
                }
                if !hex64(&version.signer) {
                    return Err(Error::invalid(format!(
                        "`{}` has an invalid signer fingerprint",
                        app.id
                    )));
                }
                if version.size == 0 || version.size > MAX_BUNDLE {
                    return Err(Error::invalid(format!(
                        "`{}` version `{}` is outside the 32 MiB cap",
                        app.id, version.version
                    )));
                }
                if version.bundle_url.trim().is_empty() {
                    return Err(Error::invalid(format!(
                        "`{}` version `{}` has no bundle URL",
                        app.id, version.version
                    )));
                }
            }
        }
        Ok(())
    }

    pub fn find(&self, id: &str) -> Option<&IndexApp> {
        self.apps.iter().find(|app| app.id == id)
    }

    pub fn find_version<'a>(
        &'a self,
        id: &str,
        version: &str,
    ) -> Option<(&'a IndexApp, &'a IndexVersion)> {
        let app = self.find(id)?;
        let found = app.versions.iter().find(|entry| entry.version == version)?;
        Some((app, found))
    }

    pub fn latest<'a>(&self, app: &'a IndexApp) -> Option<&'a IndexVersion> {
        app.versions
            .iter()
            .filter(|version| !version.yanked)
            .max_by_key(|version| version.published_at)
    }

    pub fn search(
        &self,
        source: &str,
        query: &str,
        categories: &[String],
        limit: u32,
    ) -> Vec<Value> {
        let needle = query.trim().to_lowercase();
        let mut matches: Vec<&IndexApp> = self
            .apps
            .iter()
            .filter(|app| {
                (needle.is_empty()
                    || app.id.to_lowercase().contains(&needle)
                    || app.name.to_lowercase().contains(&needle)
                    || app.summary.to_lowercase().contains(&needle))
                    && (categories.is_empty()
                        || app
                            .categories
                            .iter()
                            .any(|category| categories.contains(category)))
            })
            .collect();
        matches.sort_by_key(|app| app.name.to_lowercase());
        matches
            .into_iter()
            .take(limit.clamp(1, 100) as usize)
            .filter_map(|app| {
                let latest = self.latest(app)?.version.clone();
                Some(json!({
                    "id": app.id,
                    "name": app.name,
                    "summary": app.summary,
                    "publisher": app.publisher,
                    "categories": app.categories,
                    "latest": latest,
                    "_source": source,
                }))
            })
            .collect()
    }

    pub fn detail(&self, app: &IndexApp) -> Value {
        json!({
            "id": app.id,
            "name": app.name,
            "summary": app.summary,
            "description": app.description,
            "publisher": app.publisher,
            "license": app.license,
            "homepage": app.homepage,
            "categories": app.categories,
            "versions": app.versions.iter().map(version_view).collect::<Vec<_>>(),
        })
    }

    pub fn versions(&self, app: &IndexApp) -> Vec<Value> {
        app.versions.iter().map(version_view).collect()
    }

    pub fn updates(&self, source: &str, installed: &[Value]) -> Vec<Value> {
        let mut updates = Vec::new();
        for entry in installed {
            let Some(id) = entry.get("id").and_then(Value::as_str) else {
                continue;
            };
            let Some(current) = entry.get("version").and_then(Value::as_str) else {
                continue;
            };
            let Some(app) = self.find(id) else {
                continue;
            };
            let Some(latest) = self.latest(app) else {
                continue;
            };
            if latest.version != current {
                updates.push(json!({
                    "id": id,
                    "installed": current,
                    "latest": latest.version,
                    "channel": latest.channel,
                    "_source": source,
                }));
            }
        }
        updates
    }
}

fn version_view(version: &IndexVersion) -> Value {
    json!({
        "version": version.version,
        "channel": version.channel,
        "published-at": version.published_at,
        "sha256": version.sha256,
        "size": version.size,
        "bundle-url": version.bundle_url,
        "signer": version.signer,
        "yanked": version.yanked,
    })
}

pub fn verify_index(bytes: &[u8], signature: &str, public_key: &str) -> Result<(), Error> {
    let signature = base64::engine::general_purpose::STANDARD
        .decode(signature.trim())
        .map_err(|error| Error::invalid(format!("the index signature is not base64: {error}")))?;
    let signature: [u8; 64] = signature
        .try_into()
        .map_err(|_| Error::invalid("an Ed25519 signature is 64 bytes"))?;
    let key = hex_decode(public_key.trim())
        .ok_or_else(|| Error::invalid("the source public key is not hex"))?;
    let key: [u8; 32] = key
        .try_into()
        .map_err(|_| Error::invalid("an Ed25519 public key is 32 bytes"))?;
    let key = VerifyingKey::from_bytes(&key)
        .map_err(|error| Error::invalid(format!("the source public key is invalid: {error}")))?;
    if key.is_weak() {
        return Err(Error::invalid("the source public key is weak"));
    }
    key.verify_strict(bytes, &Signature::from_bytes(&signature))
        .map_err(|_| Error::denied("the index signature does not verify for this key"))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn hex64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn hex_decode(value: &str) -> Option<Vec<u8>> {
    if !value.len().is_multiple_of(2) {
        return None;
    }
    let mut bytes = Vec::with_capacity(value.len() / 2);
    for pair in value.as_bytes().chunks(2) {
        let text = std::str::from_utf8(pair).ok()?;
        bytes.push(u8::from_str_radix(text, 16).ok()?);
    }
    Some(bytes)
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || ".-".contains(character))
        && id.contains('.')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index() -> Index {
        Index::parse(
            br#"{
                "schema": 1,
                "store": {"id": "org.example.store", "name": "Example"},
                "apps": [{
                    "id": "org.example.notes",
                    "name": "Notes",
                    "summary": "A tiny notes app.",
                    "publisher": "Example",
                    "categories": ["productivity"],
                    "versions": [{
                        "version": "1.2.0",
                        "channel": "stable",
                        "published_at": 1,
                        "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                        "size": 10,
                        "bundle_url": "notes.anth",
                        "signer": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                        "yanked": false
                    }]
                }]
            }"#,
        )
        .expect("index")
    }

    #[test]
    fn a_minimal_index_parses_and_shapes_the_views() {
        let index = index();
        let search = index.search("org.example.store", "note", &[], 10);
        assert_eq!(search[0]["latest"], "1.2.0");
        assert_eq!(search[0]["_source"], "org.example.store");
        let app = index.find("org.example.notes").expect("app");
        assert_eq!(index.detail(app)["versions"][0]["channel"], "stable");
    }

    #[test]
    fn a_bad_schema_or_digest_is_refused() {
        let mut value = serde_json::to_value(index()).expect("json");
        value["schema"] = json!(2);
        assert!(Index::parse(value.to_string().as_bytes()).is_err());
        let mut value = serde_json::to_value(index()).expect("json");
        value["apps"][0]["versions"][0]["sha256"] = json!("short");
        assert!(Index::parse(value.to_string().as_bytes()).is_err());
    }

    #[test]
    fn updates_compare_the_installed_version() {
        let index = index();
        let updates = index.updates(
            "org.example.store",
            &[json!({"id": "org.example.notes", "version": "1.1.0"})],
        );
        assert_eq!(updates[0]["latest"], "1.2.0");
        assert!(index
            .updates(
                "org.example.store",
                &[json!({"id": "org.example.notes", "version": "1.2.0"})]
            )
            .is_empty());
    }
}
