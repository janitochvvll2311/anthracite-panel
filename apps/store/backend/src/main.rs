//! Signed catalogue, digest-pinned cache and the read-only store service.

mod cache;
mod config;
mod http;
mod index;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anth_contract::Error;
use anth_service::{ChunkStream, Handler};
use serde_json::Value;
use url::Url;

use cache::{now, Cache, Meta};
use config::{Config, Source};
use http::Http;
use index::{Index, MAX_BUNDLE, MAX_INDEX};

const CATALOG: &str = "anthracite:store/catalog@1.0.0";

struct Service {
    config: Config,
    cache: Cache,
    http: Http,
    local_root: PathBuf,
    handle: tokio::runtime::Handle,
}

impl Service {
    fn source_view(&self, source: &Source) -> Value {
        let key = self.config.normalized_key(source);
        let trusted = self.cache.index(&source.id, &key).is_some();
        self.cache.source_view(source, trusted)
    }

    fn sources(&self) -> Value {
        Value::Array(
            self.config
                .sources
                .iter()
                .map(|source| self.source_view(source))
                .collect(),
        )
    }

    async fn refresh(&self, id: &str) -> Result<Value, Error> {
        let source = self.config.source(id)?.clone();
        let key = self.config.normalized_key(&source);
        match source.kind.as_str() {
            "native" | "github" => self.refresh_remote(&source, &key).await?,
            "local" => self.refresh_local(&source, &key)?,
            other => {
                return Err(Error::invalid(format!(
                    "source `{id}` has an unknown kind `{other}`"
                )))
            }
        }
        Ok(self.source_view(&source))
    }

    async fn refresh_remote(&self, source: &Source, key: &str) -> Result<(), Error> {
        let (index_url, sig_url) = match source.kind.as_str() {
            "github" => self.github_assets(source).await?,
            _ => {
                let base = source.url.trim_end_matches('/');
                (
                    format!("{base}/index.json"),
                    format!("{base}/index.json.sig"),
                )
            }
        };
        let cached = self.cache.index(&source.id, key);
        let etag = cached.as_ref().and_then(|(_, meta)| meta.etag.clone());
        let mut headers = vec![("accept", "application/json".to_string())];
        if let Some(etag) = etag.as_deref() {
            headers.push(("if-none-match", etag.to_string()));
        }
        let response = self
            .http
            .get(&index_url, &headers, source.allow_private, MAX_INDEX)
            .await?;
        if response.status.as_u16() == 304 {
            if cached.is_some() {
                if let Some(mut meta) = self.cache.meta(&source.id) {
                    meta.refreshed_at = now();
                    self.cache
                        .store_meta(&source.id, &meta)
                        .map_err(|error| Error::unavailable(error.to_string()))?;
                }
                return Ok(());
            }
            return Err(Error::unavailable(
                "the source answered 304 without a cached index",
            ));
        }
        if !response.ok() {
            return Err(Error::unavailable(format!(
                "the source answered {} for the index",
                response.status
            )));
        }
        let signature = self
            .http
            .get(&sig_url, &[], source.allow_private, 4096)
            .await?;
        if !signature.ok() {
            return Err(Error::unavailable(format!(
                "the source answered {} for the signature",
                signature.status
            )));
        }
        let signature = String::from_utf8(signature.body)
            .map_err(|error| Error::invalid(format!("the signature is not UTF-8: {error}")))?;
        let meta = Meta {
            etag: response.header("etag").map(|value| value.to_string()),
            refreshed_at: now(),
            url: index_url,
        };
        self.cache
            .store_index(&source.id, &response.body, &signature, key, meta)
            .map_err(|error| {
                Error::invalid(format!("the catalogue {} was refused: {error}", source.id))
            })?;
        Ok(())
    }

    async fn github_assets(&self, source: &Source) -> Result<(String, String), Error> {
        let response = self
            .http
            .get(
                &source.url,
                &[
                    ("accept", "application/vnd.github+json".to_string()),
                    ("x-github-api-version", "2022-11-28".to_string()),
                ],
                source.allow_private,
                MAX_INDEX,
            )
            .await?;
        if !response.ok() {
            return Err(Error::unavailable(format!(
                "the GitHub release answered {}",
                response.status
            )));
        }
        let release: Value = serde_json::from_slice(&response.body)
            .map_err(|error| Error::invalid(format!("the GitHub release is not JSON: {error}")))?;
        let assets = release
            .get("assets")
            .and_then(Value::as_array)
            .ok_or_else(|| Error::invalid("the GitHub release has no assets"))?;
        let mut index = None;
        let mut signature = None;
        for asset in assets {
            let name = asset.get("name").and_then(Value::as_str).unwrap_or("");
            let url = asset
                .get("browser_download_url")
                .and_then(Value::as_str)
                .unwrap_or("");
            match name {
                "index.json" => index = Some(url.to_string()),
                "index.json.sig" => signature = Some(url.to_string()),
                _ => {}
            }
        }
        match (index, signature) {
            (Some(index), Some(signature)) => Ok((index, signature)),
            _ => Err(Error::invalid(
                "the GitHub release needs index.json and index.json.sig assets",
            )),
        }
    }

    fn refresh_local(&self, source: &Source, key: &str) -> Result<(), Error> {
        let dir = self.local_root.join(&source.url);
        let bytes = std::fs::read(dir.join("index.json"))
            .map_err(|error| Error::not_found(format!("reading {}: {error}", dir.display())))?;
        if bytes.len() > MAX_INDEX {
            return Err(Error::too_large("the local index is larger than 8 MiB"));
        }
        let signature = std::fs::read_to_string(dir.join("index.json.sig"))
            .map_err(|error| Error::not_found(format!("reading the signature: {error}")))?;
        let meta = Meta {
            etag: None,
            refreshed_at: now(),
            url: format!("file://{}", dir.display()),
        };
        self.cache
            .store_index(&source.id, &bytes, &signature, key, meta)
            .map_err(|error| {
                Error::invalid(format!("the catalogue {} was refused: {error}", source.id))
            })?;
        Ok(())
    }

    fn indexes(&self) -> Vec<(Source, Index)> {
        let mut indexes = Vec::new();
        for source in &self.config.sources {
            let key = self.config.normalized_key(source);
            if let Some((index, _)) = self.cache.index(&source.id, &key) {
                indexes.push((source.clone(), index));
            }
        }
        indexes
    }

    fn index_for(&self, app: &str) -> Option<(Source, Index, index::IndexApp)> {
        for (source, index) in self.indexes() {
            if let Some(found) = index.find(app).cloned() {
                return Some((source, index, found));
            }
        }
        None
    }

    fn search(&self, input: &Value) -> Value {
        let query = input.get("query").and_then(Value::as_str).unwrap_or("");
        let categories: Vec<String> = input
            .get("categories")
            .and_then(Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|entry| entry.as_str().map(|value| value.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let limit = input
            .get("limit")
            .and_then(|value| value.as_u64().or_else(|| value.as_str()?.parse().ok()))
            .unwrap_or(50) as u32;
        let mut seen = std::collections::HashSet::new();
        let mut results = Vec::new();
        for (source, index) in self.indexes() {
            for mut entry in index.search(&source.id, query, &categories, limit) {
                if let Some(object) = entry.as_object_mut() {
                    object.remove("_source");
                }
                let id = entry.get("id").and_then(Value::as_str).unwrap_or("");
                if seen.insert(id.to_string()) {
                    results.push(entry);
                }
            }
        }
        results.truncate(limit as usize);
        Value::Array(results)
    }

    fn app(&self, input: &Value) -> Result<Value, Error> {
        let id = text(input, "id")?;
        match self.index_for(id) {
            Some((_, index, app)) => Ok(index.detail(&app)),
            None => Ok(Value::Null),
        }
    }

    fn versions(&self, input: &Value) -> Result<Value, Error> {
        let id = text(input, "id")?;
        match self.index_for(id) {
            Some((_, index, app)) => Ok(Value::Array(index.versions(&app))),
            None => Ok(Value::Array(Vec::new())),
        }
    }

    fn updates(&self, input: &Value) -> Value {
        let installed = input
            .get("installed")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut seen = std::collections::HashSet::new();
        let mut updates = Vec::new();
        for (source, index) in self.indexes() {
            for mut entry in index.updates(&source.id, &installed) {
                if let Some(object) = entry.as_object_mut() {
                    object.remove("_source");
                }
                let id = entry.get("id").and_then(Value::as_str).unwrap_or("");
                if seen.insert(id.to_string()) {
                    updates.push(entry);
                }
            }
        }
        Value::Array(updates)
    }

    /// Loads or fetches one bundle, verifies the digest, and returns the bytes.
    /// The caller streams them only after this succeeds.
    async fn bundle(&self, id: &str, version: &str) -> Result<Vec<u8>, Error> {
        let (source, index, _app) = self
            .index_for(id)
            .ok_or_else(|| Error::not_found(format!("no catalogue lists `{id}`")))?;
        let (_, entry) = index
            .find_version(id, version)
            .ok_or_else(|| Error::not_found(format!("`{id}` has no version `{version}`")))?;
        if entry.yanked {
            return Err(Error::conflict(format!(
                "`{id}` version `{version}` is yanked"
            )));
        }
        if let Some(bytes) = self.cache.bundle(&entry.sha256, entry.size) {
            return Ok(bytes);
        }
        let bytes = match source.kind.as_str() {
            "local" => {
                let dir = self.local_root.join(&source.url);
                let path = safe_join(&dir, &entry.bundle_url)?;
                let metadata = std::fs::metadata(&path).map_err(|error| {
                    Error::not_found(format!("reading {}: {error}", path.display()))
                })?;
                if metadata.len() != entry.size {
                    return Err(Error::invalid(format!(
                        "`{}` is {} bytes; the index says {}",
                        path.display(),
                        metadata.len(),
                        entry.size
                    )));
                }
                std::fs::read(&path).map_err(|error| {
                    Error::unavailable(format!("reading {}: {error}", path.display()))
                })?
            }
            _ => {
                let base = match source.kind.as_str() {
                    "github" => self.github_assets(&source).await?.0,
                    _ => format!("{}/index.json", source.url.trim_end_matches('/')),
                };
                let url = resolve_bundle(&base, &entry.bundle_url)?;
                let response = self
                    .http
                    .get(&url, &[], source.allow_private, MAX_BUNDLE as usize)
                    .await?;
                if !response.ok() {
                    return Err(Error::unavailable(format!(
                        "the source answered {} for the bundle",
                        response.status
                    )));
                }
                response.body
            }
        };
        if bytes.len() as u64 != entry.size {
            return Err(Error::invalid(format!(
                "the bundle is {} bytes; the index says {}",
                bytes.len(),
                entry.size
            )));
        }
        if index::sha256_hex(&bytes) != entry.sha256 {
            return Err(Error::denied("the bundle digest does not match the index"));
        }
        self.cache.store_bundle(&entry.sha256, &bytes)?;
        Ok(bytes)
    }
}

impl Handler for Service {
    fn call(&self, interface: &str, function: &str, input: Value) -> Result<Value, Error> {
        if interface != CATALOG {
            return Err(Error::not_found(format!(
                "`{interface}` is not served by the store"
            )));
        }
        match function {
            "sources" => Ok(self.sources()),
            "refresh" => {
                let id = text(&input, "source")?;
                self.handle.block_on(self.refresh(id))
            }
            "search" => Ok(self.search(&input)),
            "app" => self.app(&input),
            "versions" => self.versions(&input),
            "check-updates" => Ok(self.updates(&input)),
            other => Err(Error::not_found(format!(
                "`{CATALOG}` has no function `{other}`"
            ))),
        }
    }

    fn stream(&self, interface: &str, function: &str, input: Value) -> Result<ChunkStream, Error> {
        if interface != CATALOG || function != "download" {
            return Err(Error::not_found(format!(
                "`{interface}#{function}` is not a stream of the store"
            )));
        }
        let id = text(&input, "id")?.to_string();
        let version = text(&input, "version")?.to_string();
        let bytes = self.handle.block_on(self.bundle(&id, &version))?;
        // Chunk after the whole bundle verified: a digest mismatch never
        // reaches the manager as a partial file.
        let mut chunks = Vec::new();
        for chunk in bytes.chunks(64 * 1024) {
            chunks.push(Ok(chunk.to_vec()));
        }
        Ok(Box::pin(futures_util::stream::iter(chunks)))
    }
}

fn text<'a>(input: &'a Value, key: &str) -> Result<&'a str, Error> {
    input
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::invalid(format!("the call needs `{key}`")))
}

fn resolve_bundle(base: &str, bundle: &str) -> Result<String, Error> {
    if bundle.starts_with("http://") || bundle.starts_with("https://") {
        return Ok(bundle.to_string());
    }
    let base = Url::parse(base)
        .map_err(|error| Error::invalid(format!("the index URL is broken: {error}")))?;
    base.join(bundle)
        .map(|url| url.to_string())
        .map_err(|error| Error::invalid(format!("the bundle URL is broken: {error}")))
}

fn safe_join(root: &Path, relative: &str) -> Result<PathBuf, Error> {
    if relative.starts_with('/')
        || relative.contains('\0')
        || relative
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(Error::invalid(format!(
            "`{relative}` is not a file under the source"
        )));
    }
    Ok(root.join(relative))
}

fn prepare_dirs(data: &Path, local_root: &Path) -> Result<(), Error> {
    use std::os::unix::fs::PermissionsExt;
    for dir in [data, local_root] {
        std::fs::create_dir_all(dir)
            .map_err(|error| Error::unavailable(format!("creating {}: {error}", dir.display())))?;
    }
    let _ = std::fs::set_permissions(data, std::fs::Permissions::from_mode(0o700));
    let _ = std::fs::set_permissions(local_root, std::fs::Permissions::from_mode(0o770));
    Ok(())
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("store-service: {error}");
        std::process::exit(if error.kind() == anth_contract::ErrorKind::Invalid {
            2
        } else {
            1
        });
    }
}

async fn run() -> Result<(), Error> {
    let socket = anth_service::socket_argument()?;
    let data = PathBuf::from(argument("--data")?);
    let shared = PathBuf::from(argument("--shared")?);
    let local_root = shared.join("store");
    prepare_dirs(&data, &local_root)?;
    let config = Config::load(&data)?;
    let cache = Cache::new(&data);
    let http = Http::new()?;
    let service = Arc::new(Service {
        config,
        cache,
        http,
        local_root,
        handle: tokio::runtime::Handle::current(),
    });
    anth_service::serve(&socket, service).await
}

fn argument(flag: &str) -> Result<String, Error> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    for pair in args.windows(2) {
        if pair[0] == flag {
            return Ok(pair[1].clone());
        }
    }
    Err(Error::invalid(format!("`{flag} <value>` is required")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bundle_path_stays_inside_the_source() {
        assert!(safe_join(Path::new("/tmp/store"), "notes.anth").is_ok());
        assert!(safe_join(Path::new("/tmp/store"), "../notes.anth").is_err());
        assert!(safe_join(Path::new("/tmp/store"), "/etc/passwd").is_err());
    }

    #[test]
    fn a_relative_bundle_url_resolves_against_the_index() {
        assert_eq!(
            resolve_bundle("https://store.example/notes/index.json", "notes.anth").expect("url"),
            "https://store.example/notes/notes.anth"
        );
        assert_eq!(
            resolve_bundle("https://store.example/index.json", "https://cdn/notes.anth")
                .expect("url"),
            "https://cdn/notes.anth"
        );
    }
}
