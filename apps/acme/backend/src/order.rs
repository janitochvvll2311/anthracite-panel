//! Orders, their events and the on-disk custody of one order.
//!
//! Every mutation rewrites `{data}/orders/<id>.json` atomically at 0600. The
//! file never holds an account key; a certificate key is written separately and
//! deleted once the certificate store accepted it.

use std::collections::HashMap;
use std::fs;
use std::io::Write as _;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anth_contract::Error;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::broadcast;

use crate::crypto::now;

pub const EVENT_CAP: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    Pending,
    Ready,
    Processing,
    Valid,
    Invalid,
    Revoked,
    IssuedUnstored,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Challenge {
    Http01,
    Dns01,
}

impl Challenge {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Http01 => "http-01",
            Self::Dns01 => "dns-01",
        }
    }

    pub fn parse(value: &str) -> Result<Self, Error> {
        match value {
            "http-01" => Ok(Self::Http01),
            "dns-01" => Ok(Self::Dns01),
            other => Err(Error::invalid(format!(
                "`{other}` is not an ACME challenge; use http-01 or dns-01"
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub seq: u64,
    pub kind: String,
    pub at: u64,
    #[serde(default)]
    pub detail: Option<String>,
}

impl Event {
    pub fn public(&self, order: &str) -> Value {
        json!({
            "order": order,
            "kind": self.kind,
            "at": self.at,
            "detail": self.detail,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub id: String,
    pub account: String,
    pub names: Vec<String>,
    pub challenge: Challenge,
    #[serde(default)]
    pub solver: Option<String>,
    #[serde(rename = "key-type")]
    pub key_type: String,
    #[serde(rename = "auto-renew")]
    pub auto_renew: bool,
    pub staging: bool,
    pub directory: String,
    pub status: Status,
    #[serde(rename = "expires-at")]
    pub expires_at: u64,
    #[serde(default)]
    pub certificate: Option<String>,
    #[serde(rename = "last-error", default)]
    pub last_error: Option<String>,
    #[serde(rename = "order-url", default)]
    pub order_url: Option<String>,
    #[serde(rename = "finalize-url", default)]
    pub finalize_url: Option<String>,
    #[serde(rename = "certificate-url", default)]
    pub certificate_url: Option<String>,
    #[serde(rename = "renew-of", default)]
    pub renew_of: Option<String>,
    #[serde(default)]
    pub confirmed: bool,
    #[serde(default)]
    pub events: Vec<Event>,
    #[serde(rename = "created-at", default)]
    pub created_at: u64,
    #[serde(rename = "updated-at", default)]
    pub updated_at: u64,
}

impl Order {
    pub fn public(&self) -> Value {
        json!({
            "id": self.id,
            "account": self.account,
            "names": self.names,
            "challenge": self.challenge.as_str(),
            "status": self.status,
            "expires-at": self.expires_at,
            "certificate": self.certificate,
            "last-error": self.last_error,
        })
    }

    pub fn dir(&self, data: &Path) -> PathBuf {
        data.join("orders").join(&self.id)
    }
}

pub struct Orders {
    data: PathBuf,
    orders: Mutex<HashMap<String, Order>>,
    events: broadcast::Sender<StreamEvent>,
    temp: Mutex<u64>,
}

#[derive(Debug, Clone)]
pub struct StreamEvent {
    pub order: String,
    pub seq: u64,
    pub public: Value,
}

impl Orders {
    pub fn load(data: &Path) -> Result<Self, Error> {
        let dir = data.join("orders");
        fs::create_dir_all(&dir)
            .map_err(|error| Error::unavailable(format!("creating {}: {error}", dir.display())))?;
        let (events, _) = broadcast::channel(256);
        let orders = Orders {
            data: data.to_path_buf(),
            orders: Mutex::new(HashMap::new()),
            events,
            temp: Mutex::new(0),
        };
        let entries = fs::read_dir(&dir)
            .map_err(|error| Error::unavailable(format!("reading {}: {error}", dir.display())))?;
        for entry in entries {
            let entry =
                entry.map_err(|error| Error::unavailable(format!("reading an order: {error}")))?;
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let Ok(raw) = fs::read(&path) else {
                continue;
            };
            let Ok(mut order) = serde_json::from_slice::<Order>(&raw) else {
                continue;
            };
            // An order the CA was still processing when the service died can
            // never be resumed safely: no nonce and no authz cursor survive.
            if matches!(
                order.status,
                Status::Pending | Status::Ready | Status::Processing
            ) {
                order.status = Status::Invalid;
                order.last_error = Some(
                    "the service restarted while this order was in flight; issue again".to_string(),
                );
                let detail = order.last_error.clone();
                push_event(&mut order, "failed", detail);
                let _ = write_order(&dir, &order);
            }
            orders
                .orders
                .lock()
                .expect("orders lock")
                .insert(order.id.clone(), order);
        }
        Ok(orders)
    }

    pub fn get(&self, id: &str) -> Result<Order, Error> {
        self.orders
            .lock()
            .expect("orders lock")
            .get(id)
            .cloned()
            .ok_or_else(|| Error::not_found(format!("no order called `{id}`")))
    }

    pub fn list(&self) -> Vec<Order> {
        let mut orders: Vec<Order> = self
            .orders
            .lock()
            .expect("orders lock")
            .values()
            .cloned()
            .collect();
        orders.sort_by_key(|order| std::cmp::Reverse(order.created_at));
        orders
    }

    pub fn insert(&self, order: Order) {
        self.orders
            .lock()
            .expect("orders lock")
            .insert(order.id.clone(), order.clone());
        let _ = self.save(&order);
    }

    pub fn save(&self, order: &Order) -> Result<(), Error> {
        write_order(&self.data.join("orders"), order)
    }

    /// Applies one mutation under the lock, bumps the timestamp, saves and
    /// returns the new order.
    pub fn update<F>(&self, id: &str, change: F) -> Result<Order, Error>
    where
        F: FnOnce(&mut Order) -> Result<(), Error>,
    {
        let mut guard = self.orders.lock().expect("orders lock");
        let order = guard
            .get_mut(id)
            .ok_or_else(|| Error::not_found(format!("no order called `{id}`")))?;
        change(order)?;
        order.updated_at = now();
        let snapshot = order.clone();
        drop(guard);
        write_order(&self.data.join("orders"), &snapshot)?;
        Ok(snapshot)
    }

    pub fn event(&self, id: &str, kind: &str, detail: Option<String>) -> Result<Order, Error> {
        let mut guard = self.orders.lock().expect("orders lock");
        let order = guard
            .get_mut(id)
            .ok_or_else(|| Error::not_found(format!("no order called `{id}`")))?;
        push_event(order, kind, detail);
        order.updated_at = now();
        let event = order.events.last().cloned().expect("an event was pushed");
        let snapshot = order.clone();
        drop(guard);
        write_order(&self.data.join("orders"), &snapshot)?;
        let _ = self.events.send(StreamEvent {
            order: id.to_string(),
            seq: event.seq,
            public: event.public(id),
        });
        Ok(snapshot)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<StreamEvent> {
        self.events.subscribe()
    }

    pub fn snapshot_events(&self, id: &str) -> Result<Vec<Event>, Error> {
        Ok(self.get(id)?.events)
    }

    pub fn temp(&self) -> u64 {
        let mut temp = self.temp.lock().expect("temp lock");
        *temp += 1;
        *temp
    }
}

fn push_event(order: &mut Order, kind: &str, detail: Option<String>) {
    let seq = order.events.last().map(|event| event.seq).unwrap_or(0) + 1;
    order.events.push(Event {
        seq,
        kind: kind.to_string(),
        at: now(),
        detail,
    });
    if order.events.len() > EVENT_CAP {
        let excess = order.events.len() - EVENT_CAP;
        order.events.drain(0..excess);
    }
}

fn write_order(dir: &Path, order: &Order) -> Result<(), Error> {
    fs::create_dir_all(dir)
        .map_err(|error| Error::unavailable(format!("creating {}: {error}", dir.display())))?;
    let path = dir.join(format!("{}.json", order.id));
    let temp = dir.join(format!(".{}.{}.tmp", order.id, now()));
    let bytes = serde_json::to_vec_pretty(order)
        .map_err(|error| Error::internal(format!("encoding the order: {error}")))?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp)
        .map_err(|error| Error::unavailable(format!("opening {}: {error}", temp.display())))?;
    file.write_all(&bytes)
        .map_err(|error| Error::unavailable(format!("writing {}: {error}", temp.display())))?;
    file.sync_all()
        .map_err(|error| Error::unavailable(format!("syncing {}: {error}", temp.display())))?;
    fs::rename(&temp, &path)
        .map_err(|error| Error::unavailable(format!("renaming {}: {error}", temp.display())))?;
    Ok(())
}

/// RFC 8555 name rules as used by the client: labels of letters, digits and
/// hyphens, a wildcard only as the whole leftmost label.
pub fn normalize_names(names: &[String]) -> Result<Vec<String>, Error> {
    if names.is_empty() {
        return Err(Error::invalid("an order needs at least one name"));
    }
    if names.len() > 100 {
        return Err(Error::invalid("an order carries at most 100 names"));
    }
    let mut seen = std::collections::HashSet::new();
    let mut normalized = Vec::new();
    for name in names {
        let name = name.trim().trim_end_matches('.').to_ascii_lowercase();
        if name.is_empty() {
            return Err(Error::invalid("a name is empty"));
        }
        let base = name.strip_prefix("*.").unwrap_or(&name);
        if base.is_empty() || base.len() > 253 {
            return Err(Error::invalid(format!(
                "`{name}` is not a fully qualified DNS name"
            )));
        }
        for label in base.split('.') {
            if label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '-')
            {
                return Err(Error::invalid(format!(
                    "`{name}` has an invalid DNS label `{label}`"
                )));
            }
        }
        if name.matches('*').count() > 1 || (name.contains('*') && !name.starts_with("*.")) {
            return Err(Error::invalid(format!(
                "`{name}` uses a wildcard outside the leftmost label"
            )));
        }
        if seen.insert(name.clone()) {
            normalized.push(name);
        }
    }
    Ok(normalized)
}

pub fn random_id() -> String {
    let mut bytes = [0u8; 8];
    getrandom::getrandom(&mut bytes).expect("the OS random source");
    hex::encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_lowercased_and_deduplicated() {
        let names = normalize_names(&[
            "Example.COM.".to_string(),
            "example.com".to_string(),
            "*.Example.com".to_string(),
        ])
        .expect("names");
        assert_eq!(names, vec!["example.com", "*.example.com"]);
    }

    #[test]
    fn a_bad_label_is_refused() {
        for name in ["", "-bad.test", "bad-.test", "a..b", "a b.test"] {
            assert!(normalize_names(&[name.to_string()]).is_err(), "{name}");
        }
        assert!(normalize_names(&["a.test".to_string(), "a.test".to_string()]).is_ok());
    }

    fn order(id: &str) -> Order {
        Order {
            id: id.to_string(),
            account: "acct".to_string(),
            names: vec!["a.test".to_string()],
            challenge: Challenge::Http01,
            solver: None,
            key_type: "ecdsa-p256".to_string(),
            auto_renew: true,
            staging: true,
            directory: "https://ca.example/dir".to_string(),
            status: Status::Pending,
            expires_at: 0,
            certificate: None,
            last_error: None,
            order_url: None,
            finalize_url: None,
            certificate_url: None,
            renew_of: None,
            confirmed: false,
            events: Vec::new(),
            created_at: now(),
            updated_at: now(),
        }
    }

    #[test]
    fn events_are_capped_and_sequence_monotonic() {
        let mut entry = order("a");
        for index in 0..(EVENT_CAP + 10) {
            push_event(&mut entry, "validated", Some(format!("{index}")));
        }
        assert_eq!(entry.events.len(), EVENT_CAP);
        assert_eq!(
            entry.events.last().expect("last").seq,
            EVENT_CAP as u64 + 10
        );
    }

    #[test]
    fn the_public_record_has_no_key_or_internal_urls() {
        let entry = order("a");
        let public = entry.public();
        assert!(public["last-error"].is_null());
        assert!(public.get("order-url").is_none());
        assert!(public.get("key-type").is_none());
    }
}
