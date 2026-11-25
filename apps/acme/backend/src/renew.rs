//! Renewal: the loop, the two-thirds/ARI decision and its backoff.
//!
//! The certificate store is the source of truth for `auto-renew`; this module
//! only acts on `source = acme`. A staging certificate is never promoted by the
//! loop: a staging order stays staging, and the operator issues a production
//! one explicitly.

use std::sync::Arc;
use std::time::Duration;

use anth_contract::Error;
use serde_json::Value;

use crate::account;
use crate::acme::AcmeClient;
use crate::crypto::now;
use crate::order::{random_id, Challenge, Order, Status};
use crate::{store, Core};

const MAX_BACKOFF: u64 = 6 * 60 * 60;

#[derive(Debug, Clone, Copy, Default)]
pub struct Backoff {
    pub failures: u32,
    pub next_attempt: u64,
}

/// 5 minutes, doubling per failure, capped at six hours.
pub fn delay(failures: u32) -> u64 {
    let exponent = failures.min(7);
    300u64.saturating_mul(1u64 << exponent).min(MAX_BACKOFF)
}

pub fn spawn(core: &Arc<Core>) {
    let core = Arc::clone(core);
    let handle = core.handle.clone();
    handle.spawn(async move {
        loop {
            let interval = core.config.renew_interval_secs.max(60);
            tokio::time::sleep(Duration::from_secs(interval)).await;
            let _ = run_once(&core).await;
        }
    });
}

pub async fn run_once(core: &Arc<Core>) -> Result<usize, Error> {
    let inventory = match store::inventory(core.broker.as_ref()).await {
        Ok(inventory) => inventory,
        // No store in the composition (or it is down): the loop is a no-op,
        // not a storm of failed orders.
        Err(_) => return Ok(0),
    };
    let mut started = 0;
    for certificate in inventory {
        if certificate.get("source").and_then(Value::as_str) != Some("acme")
            || !certificate
                .get("auto-renew")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        {
            continue;
        }
        let Some(id) = certificate.get("id").and_then(Value::as_str) else {
            continue;
        };
        // One renewal per certificate: an active order is already in flight,
        // and a valid one means the old record was replaced. Without the
        // second half the superseded record would be renewed again on every
        // pass once it crossed two thirds, burning duplicate-certificate
        // limits and growing the inventory.
        if core.orders.list().iter().any(|order| {
            order.renew_of.as_deref() == Some(id)
                && (active(order) || matches!(order.status, Status::Valid | Status::IssuedUnstored))
        }) {
            continue;
        }
        if !allowed(core, id) {
            continue;
        }
        let mapping = core
            .orders
            .list()
            .into_iter()
            .find(|order| order.certificate.as_deref() == Some(id));
        if mapping.as_ref().map(|order| order.staging).unwrap_or(false) {
            continue;
        }
        if !due(core, &certificate, mapping.as_ref()).await {
            continue;
        }
        let names = match names(&certificate) {
            Ok(names) => names,
            Err(_) => continue,
        };
        let wildcard = names.iter().any(|name| name.starts_with("*."));
        let renewal = Renewal {
            certificate: id.to_string(),
            names: names.clone(),
            account: mapping.as_ref().map(|order| order.account.clone()),
            staging: false,
            challenge: mapping
                .as_ref()
                .map(|order| order.challenge)
                .unwrap_or(if wildcard {
                    Challenge::Dns01
                } else {
                    Challenge::Http01
                }),
            solver: mapping.as_ref().and_then(|order| order.solver.clone()),
            key_type: mapping
                .as_ref()
                .map(|order| order.key_type.clone())
                .unwrap_or_else(|| "ecdsa-p256".to_string()),
        };
        match renew(core, renewal) {
            Ok(_) => {
                let mut backoff = core.renew_backoff.lock().expect("backoff lock");
                backoff.remove(id);
                started += 1;
            }
            Err(_) => {
                let mut backoff = core.renew_backoff.lock().expect("backoff lock");
                let entry = backoff.entry(id.to_string()).or_default();
                entry.failures += 1;
                entry.next_attempt = now() + delay(entry.failures);
            }
        }
    }
    Ok(started)
}

fn active(order: &Order) -> bool {
    matches!(
        order.status,
        Status::Pending | Status::Ready | Status::Processing
    )
}

fn allowed(core: &Core, id: &str) -> bool {
    core.renew_backoff
        .lock()
        .expect("backoff lock")
        .get(id)
        .map(|entry| now() >= entry.next_attempt)
        .unwrap_or(true)
}

async fn due(core: &Core, certificate: &Value, mapping: Option<&Order>) -> bool {
    let not_before = certificate
        .get("not-before")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let not_after = certificate
        .get("not-after")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if not_after == 0 {
        return false;
    }
    if let Some(order) = mapping {
        let chain = store::chain_path(&order.dir(&core.data));
        if let Ok(chain) = std::fs::read_to_string(&chain) {
            if let Ok((account, _key)) = account::load(&core.data, &order.account) {
                if let (Some(kid), Ok(directory)) =
                    (account.kid.clone(), core.config.directory(&order.directory))
                {
                    if let Ok(client) = AcmeClient::connect(&core.http, directory).await {
                        if let Some((start, _)) = client.renewal_window(&chain).await {
                            return now() >= start;
                        }
                        let _ = kid;
                    }
                }
            }
        }
    }
    now() >= two_thirds(not_before, not_after)
}

/// Two thirds of the certificate lifetime, measured from not-before.
pub fn two_thirds(not_before: u64, not_after: u64) -> u64 {
    if not_after <= not_before {
        return not_after;
    }
    not_before + (not_after - not_before) * 2 / 3
}

pub struct Renewal {
    pub certificate: String,
    pub names: Vec<String>,
    pub account: Option<String>,
    pub staging: bool,
    pub challenge: Challenge,
    pub solver: Option<String>,
    pub key_type: String,
}

/// Creates and starts one renewal order. `account` is the certificate's account
/// when this service issued it; otherwise the first production account.
pub fn renew(core: &Arc<Core>, request: Renewal) -> Result<Value, Error> {
    let Renewal {
        certificate,
        names,
        account: account_id,
        staging,
        challenge,
        solver,
        key_type,
    } = request;
    let account = match account_id {
        Some(id) => account::load(&core.data, &id)?.0,
        None => account::first_production(&core.data, &core.config).ok_or_else(|| {
            Error::not_found("no production ACME account exists; create one before renewing")
        })?,
    };
    let directory = core.config.directory(&account.directory)?.clone();
    let solver = match challenge {
        Challenge::Http01 => None,
        Challenge::Dns01 => {
            let id = solver
                .or_else(|| core.config.solvers.first().map(|solver| solver.id.clone()))
                .ok_or_else(|| Error::invalid("DNS-01 needs a configured solver"))?;
            Some(core.config.solver(&id)?.id.clone())
        }
    };
    let id = random_id();
    let created = now();
    let order = Order {
        id: id.clone(),
        account: account.id.clone(),
        names: names.clone(),
        challenge,
        solver,
        key_type,
        auto_renew: true,
        staging,
        directory: directory.url.clone(),
        status: Status::Pending,
        expires_at: created + 3_600,
        certificate: None,
        last_error: None,
        order_url: None,
        finalize_url: None,
        certificate_url: None,
        renew_of: Some(certificate.clone()),
        confirmed: false,
        events: Vec::new(),
        created_at: created,
        updated_at: created,
    };
    core.orders.insert(order);
    core.orders.event(
        &id,
        "created",
        Some(format!("renewal of {certificate}: {}", names.join(", "))),
    )?;
    crate::worker::spawn(core, &id);
    Ok(core.orders.get(&id)?.public())
}

pub fn names(certificate: &Value) -> Result<Vec<String>, Error> {
    let names: Vec<String> = certificate
        .get("names")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.as_str().map(|name| name.to_string()))
                .collect()
        })
        .unwrap_or_default();
    crate::order::normalize_names(&names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_thirds_is_measured_from_not_before() {
        assert_eq!(two_thirds(1000, 1000 + 90 * 86_400), 1000 + 60 * 86_400);
        assert_eq!(two_thirds(1000, 999), 999);
    }

    #[test]
    fn backoff_doubles_and_caps() {
        assert_eq!(delay(0), 300);
        assert_eq!(delay(1), 600);
        assert_eq!(delay(2), 1_200);
        assert_eq!(delay(10), MAX_BACKOFF);
    }
}
