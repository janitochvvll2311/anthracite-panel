//! The certificate store bridge and the order's public material.
//!
//! The certificate manager owns the inventory; the ACME client owns the CA
//! conversation and generates the key. `store-issued` is the only write path
//! into the store, through the broker grant.

use std::fs;
use std::io::Write as _;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anth_contract::Error;
use anth_service::Broker;
use serde_json::{json, Value};

pub const STORE: &str = "anthracite:certs/store@1.0.0";

pub struct Issued {
    pub chain_pem: String,
    pub key_pem: String,
}

/// Writes the PEM chain and the private key under the order directory. Both are
/// 0600; the key is unlinked after a successful store.
pub fn write_material(dir: &Path, chain_pem: &str, key_pem: &str) -> Result<(), Error> {
    fs::create_dir_all(dir)
        .map_err(|error| Error::unavailable(format!("creating {}: {error}", dir.display())))?;
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
    write_private(&dir.join("chain.pem"), chain_pem.as_bytes())?;
    write_private(&dir.join("privkey.pem"), key_pem.as_bytes())
}

pub fn chain_path(dir: &Path) -> PathBuf {
    dir.join("chain.pem")
}

pub fn key_path(dir: &Path) -> PathBuf {
    dir.join("privkey.pem")
}

pub fn remove_key(dir: &Path) {
    let _ = fs::remove_file(key_path(dir));
}

/// Calls `certs.store-issued`. The answer is the certificate record; its `id`
/// identifies the published material.
pub async fn store_issued(
    broker: Option<&Broker>,
    names: &[String],
    issued: &Issued,
    auto_renew: bool,
) -> Result<String, Error> {
    let broker = broker.ok_or_else(|| {
        Error::unavailable(
            "this composition has no certificate store; the order is issued-unstored",
        )
    })?;
    let (leaf, chain) = crate::crypto::split_chain(&issued.chain_pem);
    let input = json!({
        "cert-pem": leaf,
        "chain-pem": chain,
        "key-pem": issued.key_pem,
        "names": names,
        "auto-renew": auto_renew,
    });
    let answer = tokio::time::timeout(
        Duration::from_secs(30),
        broker.call(STORE, "store-issued", input),
    )
    .await
    .map_err(|_| Error::unavailable("the certificate store did not answer in time"))??;
    answer
        .get("id")
        .and_then(Value::as_str)
        .map(|id| id.to_string())
        .ok_or_else(|| Error::unavailable("the certificate store answered without an id"))
}

pub async fn inventory(broker: Option<&Broker>) -> Result<Vec<Value>, Error> {
    let broker = broker
        .ok_or_else(|| Error::unavailable("this composition has no certificate store to read"))?;
    let answer = tokio::time::timeout(
        Duration::from_secs(15),
        broker.call(STORE, "inventory", json!({})),
    )
    .await
    .map_err(|_| Error::unavailable("the certificate store did not answer in time"))??;
    answer
        .as_array()
        .cloned()
        .ok_or_else(|| Error::unavailable("the certificate store answered no inventory"))
}

pub async fn certificate(broker: Option<&Broker>, id: &str) -> Result<Value, Error> {
    let broker = broker
        .ok_or_else(|| Error::unavailable("this composition has no certificate store to read"))?;
    let answer = tokio::time::timeout(
        Duration::from_secs(15),
        broker.call(STORE, "get-certificate", json!({"id": id})),
    )
    .await
    .map_err(|_| Error::unavailable("the certificate store did not answer in time"))??;
    if answer.is_null() {
        return Err(Error::not_found(format!("no certificate called `{id}`")));
    }
    Ok(answer)
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), Error> {
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
