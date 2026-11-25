//! The verified cache: indexes and bundles are read through the digest, never
//! trusted because they are local.

use std::fs;
use std::io::Write as _;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anth_contract::Error;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::index::{sha256_hex, verify_index, Index, MAX_BUNDLE};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Meta {
    #[serde(default)]
    pub etag: Option<String>,
    pub refreshed_at: u64,
    pub url: String,
    #[serde(default)]
    pub sequence: u64,
}

pub struct Cache {
    data: PathBuf,
}

impl Cache {
    pub fn new(data: &Path) -> Self {
        Self {
            data: data.join("cache"),
        }
    }

    fn index_dir(&self) -> PathBuf {
        self.data.join("index")
    }

    fn bundle_dir(&self) -> PathBuf {
        self.data.join("bundles")
    }

    fn index_path(&self, source: &str) -> PathBuf {
        self.index_dir().join(format!("{source}.json"))
    }

    fn signature_path(&self, source: &str) -> PathBuf {
        self.index_dir().join(format!("{source}.json.sig"))
    }

    fn meta_path(&self, source: &str) -> PathBuf {
        self.index_dir().join(format!("{source}.meta.json"))
    }

    /// Reads and re-verifies a cached index. A corrupt cache is evicted, not
    /// served; the caller refreshes or degrades.
    pub fn index(&self, source: &str, public_key: &str) -> Option<(Index, Meta)> {
        let bytes = fs::read(self.index_path(source)).ok()?;
        let signature = fs::read_to_string(self.signature_path(source)).ok()?;
        let meta: Meta = serde_json::from_slice(&fs::read(self.meta_path(source)).ok()?).ok()?;
        if verify_index(&bytes, &signature, public_key).is_err() {
            let _ = self.evict_index(source);
            return None;
        }
        let index = Index::parse(&bytes).ok()?;
        Some((index, meta))
    }

    /// Verifies the new bytes before touching the cache; a failed signature or
    /// parse keeps the previous entry.
    pub fn store_index(
        &self,
        source: &str,
        bytes: &[u8],
        signature: &str,
        public_key: &str,
        meta: Meta,
    ) -> Result<Index, Error> {
        verify_index(bytes, signature, public_key)?;
        let index = Index::parse(bytes)?;
        if let Some(cached) = self.meta(source) {
            if index.sequence < cached.sequence {
                return Err(Error::conflict(format!(
                    "the catalogue sequence {} is older than the cached {}; refusing a replay",
                    index.sequence, cached.sequence
                )));
            }
        }
        let mut meta = meta;
        meta.sequence = index.sequence;
        fs::create_dir_all(self.index_dir()).map_err(|error| {
            Error::unavailable(format!("creating {}: {error}", self.index_dir().display()))
        })?;
        write_private(&self.index_path(source), bytes)?;
        write_private(&self.signature_path(source), signature.as_bytes())?;
        let meta = serde_json::to_vec_pretty(&meta)
            .map_err(|error| Error::internal(format!("encoding the meta: {error}")))?;
        write_private(&self.meta_path(source), &meta)?;
        Ok(index)
    }

    pub fn store_meta(&self, source: &str, meta: &Meta) -> Result<(), Error> {
        let bytes = serde_json::to_vec_pretty(meta)
            .map_err(|error| Error::internal(format!("encoding the meta: {error}")))?;
        write_private(&self.meta_path(source), &bytes)
    }

    pub fn meta(&self, source: &str) -> Option<Meta> {
        serde_json::from_slice(&fs::read(self.meta_path(source)).ok()?).ok()
    }

    pub fn evict_index(&self, source: &str) -> Result<(), Error> {
        for path in [
            self.index_path(source),
            self.signature_path(source),
            self.meta_path(source),
        ] {
            let _ = fs::remove_file(path);
        }
        Ok(())
    }

    pub fn bundle(&self, digest: &str, size: u64) -> Option<Vec<u8>> {
        if size == 0 || size > MAX_BUNDLE {
            return None;
        }
        let path = self.bundle_dir().join(format!("{digest}.anth"));
        let bytes = fs::read(&path).ok()?;
        if bytes.len() as u64 != size || sha256_hex(&bytes) != digest {
            let _ = fs::remove_file(path);
            return None;
        }
        Some(bytes)
    }

    pub fn store_bundle(&self, digest: &str, bytes: &[u8]) -> Result<(), Error> {
        if sha256_hex(bytes) != digest {
            return Err(Error::invalid("the bundle digest does not match"));
        }
        fs::create_dir_all(self.bundle_dir()).map_err(|error| {
            Error::unavailable(format!("creating {}: {error}", self.bundle_dir().display()))
        })?;
        write_private(&self.bundle_dir().join(format!("{digest}.anth")), bytes)
    }

    /// Source metadata for the WIT record: trust is the signature check, age is
    /// the cache timestamp.
    pub fn source_view(&self, source: &crate::config::Source, trusted: bool) -> Value {
        let refreshed_at = self
            .meta(&source.id)
            .map(|meta| meta.refreshed_at)
            .unwrap_or(0);
        serde_json::json!({
            "id": source.id,
            "name": source.name,
            "kind": source.kind,
            "url": source.url,
            "trusted": trusted,
            "refreshed-at": refreshed_at,
        })
    }
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_secs())
        .unwrap_or(0)
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            Error::unavailable(format!("creating {}: {error}", parent.display()))
        })?;
    }
    let unique = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_nanos())
        .unwrap_or(0);
    let temp = path.with_extension(format!("tmp{}-{unique}", std::process::id()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&temp)
        .map_err(|error| Error::unavailable(format!("opening {}: {error}", temp.display())))?;
    file.write_all(bytes)
        .map_err(|error| Error::unavailable(format!("writing {}: {error}", temp.display())))?;
    file.sync_all()
        .map_err(|error| Error::unavailable(format!("syncing {}: {error}", temp.display())))?;
    fs::rename(&temp, path)
        .map_err(|error| Error::unavailable(format!("renaming {}: {error}", temp.display())))
}
