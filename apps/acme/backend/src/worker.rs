//! The issuance worker: one task per order. It owns the CA conversation, the
//! challenge lifecycle and the handoff to the certificate store. A failure sets
//! `invalid` and one `failed` event; it never retries silently.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anth_contract::Error;
use serde_json::Value;

use crate::account;
use crate::acme::AcmeClient;
use crate::config::Solver;
use crate::crypto::{certificate_key, csr, now, split_chain};
use crate::order::{Challenge, Order, Status};
use crate::solver::{self, DnsRecord};
use crate::store::{self, Issued};
use crate::Core;

const AUTHZ_DEADLINE: u64 = 300;
const FINALIZE_DEADLINE: u64 = 300;
const CONFIRM_DEADLINE: u64 = 3_600;

pub fn spawn(core: &Arc<Core>, id: &str) {
    let core = Arc::clone(core);
    let id = id.to_string();
    let handle = core.handle.clone();
    handle.spawn(async move {
        if let Err(error) = run(&core, &id).await {
            let message = error.to_string();
            let _ = core.orders.update(&id, |order| {
                order.status = Status::Invalid;
                order.last_error = Some(message.clone());
                Ok(())
            });
            let _ = core.orders.event(&id, "failed", Some(message));
        }
    });
}

struct Run {
    tokens: Vec<PathBuf>,
    dns: Vec<(Solver, DnsRecord)>,
}

impl Run {
    fn new() -> Self {
        Self {
            tokens: Vec::new(),
            dns: Vec::new(),
        }
    }

    async fn cleanup(&mut self) {
        for path in std::mem::take(&mut self.tokens) {
            solver::remove_http_token(&path);
        }
        for (solver, record) in std::mem::take(&mut self.dns) {
            let _ = solver::cleanup(&solver, &record).await;
        }
    }
}

async fn run(core: &Arc<Core>, id: &str) -> Result<(), Error> {
    let mut run = Run::new();
    let result = execute(core, id, &mut run).await;
    run.cleanup().await;
    result
}

async fn execute(core: &Arc<Core>, id: &str, run: &mut Run) -> Result<(), Error> {
    let order = core.orders.get(id)?;
    if matches!(order.status, Status::Revoked) {
        return Err(Error::conflict("the order was already revoked"));
    }
    let (account, key) = account::load(&core.data, &order.account)?;
    let directory = core.config.directory(&order.directory)?.clone();
    let kid = account
        .kid
        .clone()
        .ok_or_else(|| Error::invalid("the account has no ACME URL"))?;
    let client = AcmeClient::connect(&core.http, &directory).await?;

    let (order_url, document) = client.new_order(&key, &kid, &order.names).await?;
    let finalize_url = document
        .get("finalize")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::unavailable("the CA returned an order without `finalize`"))?
        .to_string();
    let expires_at = document
        .get("expires")
        .and_then(Value::as_str)
        .and_then(crate::acme::parse_timestamp)
        .unwrap_or_else(|| now() + 3_600);
    core.orders.update(id, |order| {
        order.order_url = Some(order_url.clone());
        order.finalize_url = Some(finalize_url.clone());
        order.expires_at = expires_at;
        order.status = Status::Pending;
        Ok(())
    })?;

    let authorizations: Vec<String> = document
        .get("authorizations")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.as_str().map(|url| url.to_string()))
                .collect()
        })
        .unwrap_or_default();
    if authorizations.is_empty() {
        return Err(Error::unavailable(
            "the CA returned an order without authorizations",
        ));
    }

    let solver = dns_solver(core, &order)?;
    let mut challenges = Vec::new();
    for url in &authorizations {
        let authorization = client.get(url, &key, &kid).await?;
        let status = authorization
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("pending");
        if status == "valid" {
            continue;
        }
        if status == "invalid" {
            return Err(Error::invalid(format!(
                "the authorization {url} is already invalid"
            )));
        }
        let (kind, challenge_url, token) = pick_challenge(&authorization, &order)?;
        // One authorization covers one identifier; a multi-SAN order has one
        // per name, so the self-check and the TXT record use that name, not the
        // first one in the request.
        let identifier = authorization
            .pointer("/identifier/value")
            .and_then(Value::as_str)
            .unwrap_or(&order.names[0])
            .to_string();
        let key_authorization = format!("{token}.{}", key.thumbprint());
        let mut record = None;
        match kind {
            Challenge::Http01 => {
                let path = solver::write_http_token(&core.http01_root, &token, &key_authorization)?;
                run.tokens.push(path);
                solver::self_check(
                    &core.http,
                    &identifier,
                    core.config.http01_port,
                    &token,
                    &key_authorization,
                )
                .await?;
            }
            Challenge::Dns01 => {
                let solver = solver
                    .as_ref()
                    .ok_or_else(|| Error::invalid("DNS-01 needs a configured solver"))?;
                let txt = solver::txt_record(&identifier, &key_authorization);
                solver::present(solver, &txt).await?;
                run.dns.push((solver.clone(), txt.clone()));
                record = Some(txt);
            }
        }
        let detail = match &record {
            Some(txt) if solver.as_ref().map(|solver| solver.kind.as_str()) == Some("manual") => {
                Some(format!(
                    "create TXT {} with {} and confirm when it resolves",
                    txt.name.trim_end_matches('.'),
                    txt.value
                ))
            }
            Some(txt) => Some(format!(
                "TXT {} published by the {} solver",
                txt.name.trim_end_matches('.'),
                solver
                    .as_ref()
                    .map(|solver| solver.kind.as_str())
                    .unwrap_or("dns")
            )),
            None => Some("HTTP-01 token published and self-checked".to_string()),
        };
        core.orders.event(id, "challenge-ready", detail)?;
        challenges.push((challenge_url, token, record));
    }

    if order.challenge == Challenge::Dns01
        && solver
            .as_ref()
            .map(|solver| solver.kind.as_str() == "manual")
            .unwrap_or(false)
    {
        wait_for_confirm(core, id).await?;
    }

    for (url, _, _) in &challenges {
        client.trigger_challenge(url, &key, &kid).await?;
    }

    for (url, _, _) in &challenges {
        poll_authorization(&client, url, &key, &kid, now() + AUTHZ_DEADLINE).await?;
    }
    core.orders.event(
        id,
        "validated",
        Some("every authorization is valid".to_string()),
    )?;
    core.orders.update(id, |order| {
        order.status = Status::Processing;
        Ok(())
    })?;

    let cert_key = certificate_key(&order.key_type)?;
    let request = csr(&cert_key.key, &order.names)?;
    let material = order.dir(&core.data);
    store::write_material(&material, "", &cert_key.pem)?;

    let finalized = client.finalize(&finalize_url, &key, &kid, &request).await?;
    if finalized.get("status").and_then(Value::as_str) == Some("invalid") {
        return Err(order_problem(&finalized));
    }
    let order_state = if certificate_url(&finalized).is_some() {
        finalized
    } else {
        poll_order(&client, &order_url, &key, &kid, now() + FINALIZE_DEADLINE).await?
    };
    let certificate_url = certificate_url(&order_state)
        .ok_or_else(|| Error::unavailable("the CA reports valid without a certificate URL"))?;
    let chain_pem = client.download(&certificate_url, &key, &kid).await?;
    let (leaf, chain) = split_chain(&chain_pem);
    let fullchain = if chain.trim().is_empty() {
        format!("{}\n", leaf.trim())
    } else {
        format!("{}\n{}\n", leaf.trim(), chain.trim())
    };
    store::write_material(&material, &fullchain, &cert_key.pem)?;

    let issued = Issued {
        chain_pem: fullchain.clone(),
        key_pem: cert_key.pem.clone(),
    };
    let kind = if order.renew_of.is_some() {
        "renewed"
    } else {
        "issued"
    };
    match store::store_issued(
        core.broker.as_ref(),
        &order.names,
        &issued,
        order.auto_renew,
    )
    .await
    {
        Ok(certificate) => {
            core.orders.update(id, |order| {
                order.status = Status::Valid;
                order.certificate = Some(certificate.clone());
                order.last_error = None;
                Ok(())
            })?;
            store::remove_key(&material);
            core.orders.event(
                id,
                kind,
                Some(format!("certificate {certificate} stored and published")),
            )?;
        }
        Err(error) => {
            let warning = format!("issued but not stored: {error}");
            core.orders.update(id, |order| {
                order.status = Status::IssuedUnstored;
                order.last_error = Some(warning.clone());
                Ok(())
            })?;
            core.orders.event(id, kind, Some(warning))?;
        }
    }
    Ok(())
}

fn dns_solver(core: &Core, order: &Order) -> Result<Option<Solver>, Error> {
    if order.challenge != Challenge::Dns01 {
        return Ok(None);
    }
    let id = order
        .solver
        .as_deref()
        .or_else(|| core.config.solvers.first().map(|solver| solver.id.as_str()))
        .ok_or_else(|| Error::invalid("DNS-01 needs at least one solver in the config"))?;
    Ok(Some(core.config.solver(id)?.clone()))
}

fn pick_challenge(
    authorization: &Value,
    order: &Order,
) -> Result<(Challenge, String, String), Error> {
    let kind = order.challenge.as_str();
    let entries = authorization
        .get("challenges")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::unavailable("the authorization has no challenges"))?;
    let challenge = entries
        .iter()
        .find(|entry| entry.get("type").and_then(Value::as_str) == Some(kind))
        .ok_or_else(|| {
            Error::invalid(format!(
                "the CA did not offer {kind} for this authorization"
            ))
        })?;
    let url = challenge
        .get("url")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::unavailable("the challenge has no URL"))?
        .to_string();
    let token = challenge
        .get("token")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::unavailable("the challenge has no token"))?
        .to_string();
    Ok((order.challenge, url, token))
}

async fn wait_for_confirm(core: &Core, id: &str) -> Result<(), Error> {
    let deadline = now() + CONFIRM_DEADLINE;
    loop {
        let order = core.orders.get(id)?;
        if order.confirmed {
            return Ok(());
        }
        if !matches!(order.status, Status::Pending | Status::Ready) {
            return Err(Error::invalid(format!(
                "the order became `{:?}` before the operator confirmed the DNS record",
                order.status
            )));
        }
        if now() >= deadline {
            return Err(Error::unavailable(
                "the DNS record was not confirmed before the challenge timed out",
            ));
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

async fn poll_authorization(
    client: &AcmeClient,
    url: &str,
    key: &crate::crypto::AccountKey,
    kid: &str,
    deadline: u64,
) -> Result<(), Error> {
    loop {
        let value = client.get(url, key, kid).await?;
        match value.get("status").and_then(Value::as_str).unwrap_or("") {
            "valid" => return Ok(()),
            "invalid" => {
                let detail = value
                    .get("challenges")
                    .and_then(Value::as_array)
                    .and_then(|entries| {
                        entries
                            .iter()
                            .find_map(|entry| entry.pointer("/error/detail"))
                    })
                    .and_then(Value::as_str)
                    .unwrap_or("the CA refused the challenge");
                return Err(Error::invalid(format!(
                    "the authorization failed: {detail}"
                )));
            }
            _ => {}
        }
        if now() >= deadline {
            return Err(Error::unavailable(
                "the CA did not validate the challenge before the deadline",
            ));
        }
        tokio::time::sleep(retry_delay(client)).await;
    }
}

async fn poll_order(
    client: &AcmeClient,
    url: &str,
    key: &crate::crypto::AccountKey,
    kid: &str,
    deadline: u64,
) -> Result<Value, Error> {
    loop {
        let value = client.get(url, key, kid).await?;
        match value.get("status").and_then(Value::as_str).unwrap_or("") {
            "valid" => return Ok(value),
            "invalid" => return Err(order_problem(&value)),
            _ => {}
        }
        if now() >= deadline {
            return Err(Error::unavailable(
                "the CA did not finalize the order before the deadline",
            ));
        }
        tokio::time::sleep(retry_delay(client)).await;
    }
}

fn retry_delay(client: &AcmeClient) -> Duration {
    client
        .retry_after()
        .map(|seconds| Duration::from_secs(seconds.clamp(1, 30)))
        .unwrap_or(Duration::from_secs(2))
}

fn certificate_url(order: &Value) -> Option<String> {
    order
        .get("certificate")
        .and_then(Value::as_str)
        .map(|url| url.to_string())
}

fn order_problem(order: &Value) -> Error {
    let detail = order
        .pointer("/error/detail")
        .and_then(Value::as_str)
        .unwrap_or("the CA marked the order invalid");
    Error::invalid(format!("the order failed: {detail}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_manual_confirmation_timeout_is_explicit() {
        let text =
            Error::unavailable("the DNS record was not confirmed before the challenge timed out")
                .to_string();
        assert!(text.contains("not confirmed"));
    }
}
