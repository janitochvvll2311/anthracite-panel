//! ACME accounts and their key custody.
//!
//! One directory per account. The account key is PKCS#8 PEM at 0600 under
//! `{data}/accounts/<id>/`; no route, event or audit detail ever includes it.
//! The public record omits the CA's `kid` too: URL-shaped account handles are
//! internal.

use std::fs;
use std::io::Write as _;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use anth_contract::Error;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::acme::AcmeClient;
use crate::config::Config;
use crate::crypto::{now, AccountKey};
use crate::http::Http;
use crate::order::random_id;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountMeta {
    pub id: String,
    pub directory: String,
    #[serde(default)]
    pub contact: Vec<String>,
    pub status: String,
    #[serde(rename = "created-at")]
    pub created_at: u64,
    #[serde(rename = "key-type")]
    pub key_type: String,
    #[serde(default)]
    pub kid: Option<String>,
}

impl AccountMeta {
    pub fn public(&self) -> Value {
        json!({
            "id": self.id,
            "directory": self.directory,
            "contact": self.contact,
            "status": self.status,
            "created-at": self.created_at,
            "key-type": self.key_type,
        })
    }

    pub fn dir(&self, data: &Path) -> PathBuf {
        data.join("accounts").join(&self.id)
    }
}

/// Creates the account through the CA and stores its key and metadata. The CA
/// may answer 200 with an existing account URL (same key); that is still one
/// local record per local key.
pub async fn create(
    data: &Path,
    config: &Config,
    http: &Http,
    directory_url: &str,
    contact: &[String],
    key_type: &str,
    eab: Option<Value>,
) -> Result<Value, Error> {
    let directory = config.directory(directory_url)?.clone();
    let contact = normalize_contact(contact)?;
    let key = AccountKey::generate(key_type)?;
    let client = AcmeClient::connect(http, &directory).await?;
    let (kid, account) = client.new_account(&key, &contact, eab).await?;
    let id = random_id();
    let meta = AccountMeta {
        id: id.clone(),
        directory: directory_url.to_string(),
        contact,
        status: account
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("valid")
            .to_string(),
        created_at: now(),
        key_type: key.key_type().to_string(),
        kid: Some(kid),
    };
    let dir = meta.dir(data);
    fs::create_dir_all(&dir)
        .map_err(|error| Error::unavailable(format!("creating {}: {error}", dir.display())))?;
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700));
    let key_pem = key.to_pem()?;
    write_private(&dir.join("account.key.pem"), key_pem.as_bytes())?;
    let bytes = serde_json::to_vec_pretty(&meta)
        .map_err(|error| Error::internal(format!("encoding the account: {error}")))?;
    write_private(&dir.join("account.json"), &bytes)?;
    Ok(meta.public())
}

pub fn load(data: &Path, id: &str) -> Result<(AccountMeta, AccountKey), Error> {
    if id.is_empty() || !id.chars().all(|character| character.is_ascii_hexdigit()) {
        return Err(Error::not_found(format!("no account called `{id}`")));
    }
    let dir = data.join("accounts").join(id);
    let raw = fs::read(dir.join("account.json"))
        .map_err(|_| Error::not_found(format!("no account called `{id}`")))?;
    let meta: AccountMeta = serde_json::from_slice(&raw)
        .map_err(|error| Error::internal(format!("account `{id}`: {error}")))?;
    let pem = fs::read_to_string(dir.join("account.key.pem"))
        .map_err(|error| Error::unavailable(format!("reading the key of `{id}`: {error}")))?;
    let key = AccountKey::from_pem(&pem)?;
    Ok((meta, key))
}

pub fn list(data: &Path) -> Vec<Value> {
    let dir = data.join("accounts");
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut accounts = Vec::new();
    for entry in entries.flatten() {
        let Ok(raw) = fs::read(entry.path().join("account.json")) else {
            continue;
        };
        let Ok(meta) = serde_json::from_slice::<AccountMeta>(&raw) else {
            continue;
        };
        accounts.push(meta);
    }
    accounts.sort_by_key(|account| account.created_at);
    accounts.iter().map(AccountMeta::public).collect()
}

pub fn first_production(data: &Path, config: &Config) -> Option<AccountMeta> {
    let dir = data.join("accounts");
    let entries = fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let Ok(raw) = fs::read(entry.path().join("account.json")) else {
            continue;
        };
        let Ok(meta) = serde_json::from_slice::<AccountMeta>(&raw) else {
            continue;
        };
        let Some(directory) = config
            .directories
            .iter()
            .find(|directory| directory.url == meta.directory)
        else {
            continue;
        };
        if !directory.staging {
            return Some(meta);
        }
    }
    None
}

/// `user@example.com` becomes `mailto:user@example.com`; anything that is not a
/// mailto contact is refused. An empty list is allowed by RFC 8555.
pub fn normalize_contact(contact: &[String]) -> Result<Vec<String>, Error> {
    if contact.len() > 10 {
        return Err(Error::invalid("an account carries at most 10 contacts"));
    }
    let mut normalized = Vec::new();
    for entry in contact {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let address = entry.strip_prefix("mailto:").unwrap_or(entry);
        if entry.contains(':') && !entry.starts_with("mailto:") {
            return Err(Error::invalid(format!("`{entry}` is not a mailto contact")));
        }
        if address.contains(char::is_whitespace)
            || !address.contains('@')
            || address.starts_with('@')
            || address.ends_with('@')
        {
            return Err(Error::invalid(format!("`{entry}` is not an email address")));
        }
        let contact = format!("mailto:{address}");
        if !normalized.contains(&contact) {
            normalized.push(contact);
        }
    }
    Ok(normalized)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contacts_are_normalized_to_mailto() {
        assert_eq!(
            normalize_contact(&["ops@example.com".to_string(), "mailto:a@b.test".to_string()])
                .expect("contacts"),
            vec![
                "mailto:ops@example.com".to_string(),
                "mailto:a@b.test".to_string()
            ]
        );
    }

    #[test]
    fn a_non_mailto_contact_is_refused() {
        assert!(normalize_contact(&["tel:+1555".to_string()]).is_err());
        assert!(normalize_contact(&["not-an-email".to_string()]).is_err());
    }

    #[test]
    fn a_public_account_record_never_contains_the_key_or_kid() {
        let meta = AccountMeta {
            id: "a".to_string(),
            directory: "https://ca.example/dir".to_string(),
            contact: vec!["mailto:ops@example.com".to_string()],
            status: "valid".to_string(),
            created_at: 1,
            key_type: "ecdsa-p256".to_string(),
            kid: Some("https://ca.example/account/1".to_string()),
        };
        let public = meta.public().to_string();
        assert!(!public.contains("account/1"));
        assert!(!public.contains("PRIVATE"));
    }
}
