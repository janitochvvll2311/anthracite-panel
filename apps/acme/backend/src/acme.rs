//! The RFC 8555 conversation: directory, nonces, JWS requests and the order
//! resources. This module is HTTP and JSON only; the state machine lives in
//! `order.rs` and `worker.rs`.

use std::sync::Mutex;

use anth_contract::Error;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use http::Method;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::config::Directory;
use crate::crypto::{external_account_binding, AccountKey, Jws};
use crate::http::{Http, Response};

#[derive(Debug, Clone, Deserialize)]
pub struct DirectoryDocument {
    #[serde(rename = "newNonce")]
    pub new_nonce: String,
    #[serde(rename = "newAccount")]
    pub new_account: String,
    #[serde(rename = "newOrder")]
    pub new_order: String,
    #[serde(rename = "revokeCert", default)]
    pub revoke_cert: Option<String>,
    #[serde(default)]
    pub meta: Value,
}

impl DirectoryDocument {
    pub fn renewal_info(&self) -> Option<&str> {
        self.meta.get("renewalInfo").and_then(Value::as_str)
    }
}

#[derive(Debug, Deserialize)]
struct Problem {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    detail: String,
}

pub struct AcmeClient {
    http: Http,
    pub directory: Directory,
    pub document: DirectoryDocument,
    nonce: Mutex<Option<String>>,
    retry_after: Mutex<Option<u64>>,
}

impl AcmeClient {
    pub async fn connect(http: &Http, directory: &Directory) -> Result<Self, Error> {
        let response = http
            .request(
                Method::GET,
                &directory.url,
                &[("accept", "application/json".to_string())],
                Vec::new(),
                directory.allow_private,
            )
            .await?;
        if !response.status.is_success() {
            return Err(problem(&response));
        }
        let document: DirectoryDocument =
            serde_json::from_slice(&response.body).map_err(|error| {
                Error::invalid(format!(
                    "the directory {} is not RFC 8555 JSON: {error}",
                    directory.url
                ))
            })?;
        Ok(Self {
            http: http.clone(),
            directory: directory.clone(),
            document,
            nonce: Mutex::new(None),
            retry_after: Mutex::new(None),
        })
    }

    fn allow_private(&self) -> bool {
        self.directory.allow_private
    }

    async fn nonce(&self) -> Result<String, Error> {
        if let Some(nonce) = self.nonce.lock().expect("nonce lock").take() {
            return Ok(nonce);
        }
        for method in [Method::HEAD, Method::GET] {
            let response = self
                .http
                .request(
                    method,
                    &self.document.new_nonce,
                    &[],
                    Vec::new(),
                    self.allow_private(),
                )
                .await?;
            if let Some(nonce) = response.header("replay-nonce") {
                return Ok(nonce.to_string());
            }
        }
        Err(Error::unavailable(
            "the CA did not return a Replay-Nonce from newNonce",
        ))
    }

    fn remember(&self, response: &Response) {
        if let Some(nonce) = response.header("replay-nonce") {
            *self.nonce.lock().expect("nonce lock") = Some(nonce.to_string());
        }
        if let Some(seconds) = response
            .header("retry-after")
            .and_then(|value| value.parse::<u64>().ok())
        {
            *self.retry_after.lock().expect("retry lock") = Some(seconds);
        }
    }

    /// The last `Retry-After` in seconds, consumed by the polling loop.
    pub fn retry_after(&self) -> Option<u64> {
        self.retry_after.lock().expect("retry lock").take()
    }

    async fn post(
        &self,
        url: &str,
        key: &AccountKey,
        kid: Option<&str>,
        payload: &[u8],
        accept: &str,
    ) -> Result<Response, Error> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            let nonce = self.nonce().await?;
            let jws = Jws::new(key, url, &nonce, kid, payload)?;
            let body = serde_json::to_vec(&jws.body())
                .map_err(|error| Error::internal(format!("encoding the JWS: {error}")))?;
            let response = self
                .http
                .request(
                    Method::POST,
                    url,
                    &[
                        ("content-type", "application/jose+json".to_string()),
                        ("accept", accept.to_string()),
                    ],
                    body,
                    self.allow_private(),
                )
                .await?;
            self.remember(&response);
            if response.status.is_success() {
                return Ok(response);
            }
            let error = problem(&response);
            let bad_nonce = response.status == 400
                && serde_json::from_slice::<Problem>(&response.body)
                    .map(|problem| problem.kind.ends_with("badNonce"))
                    .unwrap_or(false);
            if bad_nonce && attempt < 3 {
                continue;
            }
            return Err(error);
        }
    }

    pub async fn new_account(
        &self,
        key: &AccountKey,
        contact: &[String],
        eab: Option<Value>,
    ) -> Result<(String, Value), Error> {
        let mut payload = json!({
            "termsOfServiceAgreed": true,
            "contact": contact,
        });
        if let Some(eab) = eab {
            let kid = eab
                .get("kid")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::invalid("the EAB credentials need `kid`"))?;
            let secret = eab
                .get("hmac-key")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::invalid("the EAB credentials need `hmac-key`"))?;
            let binding =
                external_account_binding(&self.document.new_account, kid, secret, &key.jwk())?;
            payload["externalAccountBinding"] = binding;
        }
        let body = serde_json::to_vec(&payload)
            .map_err(|error| Error::internal(format!("encoding newAccount: {error}")))?;
        let response = self
            .post(
                &self.document.new_account,
                key,
                None,
                &body,
                "application/json",
            )
            .await?;
        let account: Value = serde_json::from_slice(&response.body).map_err(|error| {
            Error::unavailable(format!("the CA answered newAccount without JSON: {error}"))
        })?;
        let location = response.header("location").ok_or_else(|| {
            Error::unavailable("the CA did not return the account URL in Location")
        })?;
        Ok((location.to_string(), account))
    }

    pub async fn new_order(
        &self,
        key: &AccountKey,
        kid: &str,
        names: &[String],
    ) -> Result<(String, Value), Error> {
        let identifiers: Vec<Value> = names
            .iter()
            .map(|name| json!({"type": "dns", "value": name}))
            .collect();
        let body = serde_json::to_vec(&json!({"identifiers": identifiers}))
            .map_err(|error| Error::internal(format!("encoding newOrder: {error}")))?;
        let response = self
            .post(
                &self.document.new_order,
                key,
                Some(kid),
                &body,
                "application/json",
            )
            .await?;
        let order: Value = serde_json::from_slice(&response.body).map_err(|error| {
            Error::unavailable(format!("the CA answered newOrder without JSON: {error}"))
        })?;
        let location = response
            .header("location")
            .ok_or_else(|| Error::unavailable("the CA did not return the order URL"))?;
        Ok((location.to_string(), order))
    }

    pub async fn get(&self, url: &str, key: &AccountKey, kid: &str) -> Result<Value, Error> {
        let response = self
            .post(url, key, Some(kid), b"", "application/json")
            .await?;
        serde_json::from_slice(&response.body)
            .map_err(|error| Error::unavailable(format!("{url} did not answer JSON: {error}")))
    }

    pub async fn trigger_challenge(
        &self,
        url: &str,
        key: &AccountKey,
        kid: &str,
    ) -> Result<Value, Error> {
        let response = self
            .post(url, key, Some(kid), b"{}", "application/json")
            .await?;
        serde_json::from_slice(&response.body).map_err(|error| {
            Error::unavailable(format!("the challenge answer is not JSON: {error}"))
        })
    }

    pub async fn finalize(
        &self,
        url: &str,
        key: &AccountKey,
        kid: &str,
        csr: &[u8],
    ) -> Result<Value, Error> {
        let body = serde_json::to_vec(&json!({"csr": URL_SAFE_NO_PAD.encode(csr)}))
            .map_err(|error| Error::internal(format!("encoding finalize: {error}")))?;
        let response = self
            .post(url, key, Some(kid), &body, "application/json")
            .await?;
        serde_json::from_slice(&response.body)
            .map_err(|error| Error::unavailable(format!("finalize did not answer JSON: {error}")))
    }

    pub async fn download(&self, url: &str, key: &AccountKey, kid: &str) -> Result<String, Error> {
        let response = self
            .post(
                url,
                key,
                Some(kid),
                b"",
                "application/pem-certificate-chain",
            )
            .await?;
        let body = String::from_utf8(response.body)
            .map_err(|error| Error::invalid(format!("the certificate is not UTF-8: {error}")))?;
        if !body.contains("-----BEGIN CERTIFICATE-----") {
            return Err(Error::unavailable(
                "the CA answered the certificate URL without a PEM chain",
            ));
        }
        Ok(body)
    }

    pub async fn revoke(
        &self,
        key: &AccountKey,
        kid: &str,
        certificate_pem: &str,
        reason: u8,
    ) -> Result<(), Error> {
        let url = self
            .document
            .revoke_cert
            .as_deref()
            .ok_or_else(|| Error::invalid("this CA directory has no revokeCert endpoint"))?;
        let der = crate::crypto::leaf_der(certificate_pem)?;
        let body = serde_json::to_vec(&json!({
            "certificate": URL_SAFE_NO_PAD.encode(der),
            "reason": reason,
        }))
        .map_err(|error| Error::internal(format!("encoding revokeCert: {error}")))?;
        self.post(url, key, Some(kid), &body, "application/json")
            .await?;
        Ok(())
    }

    pub async fn renewal_window(&self, certificate_pem: &str) -> Option<(u64, u64)> {
        let base = self.document.renewal_info()?;
        let der = crate::crypto::leaf_der(certificate_pem).ok()?;
        let id = URL_SAFE_NO_PAD.encode(der);
        let url = format!("{}/{}", base.trim_end_matches('/'), id);
        let response = self
            .http
            .request(
                Method::GET,
                &url,
                &[("accept", "application/json".to_string())],
                Vec::new(),
                self.allow_private(),
            )
            .await
            .ok()?;
        if !response.status.is_success() {
            return None;
        }
        let value: Value = serde_json::from_slice(&response.body).ok()?;
        let window = value.get("suggestedWindow")?;
        let start = parse_timestamp(window.get("start")?.as_str()?)?;
        let end = parse_timestamp(window.get("end")?.as_str()?)?;
        Some((start, end))
    }
}

fn problem(response: &Response) -> Error {
    let (kind, detail) = match serde_json::from_slice::<Problem>(&response.body) {
        Ok(problem) => (problem.kind, problem.detail),
        Err(_) => (
            String::new(),
            String::from_utf8_lossy(&response.body).trim().to_string(),
        ),
    };
    let message = if detail.is_empty() {
        format!("the CA answered {} at {}", response.status, response.url)
    } else {
        format!(
            "the CA answered {} at {}: {detail}",
            response.status, response.url
        )
    };
    if response.status == 429 || kind.ends_with("rateLimited") {
        Error::rate_limited(message)
    } else if kind.ends_with("badNonce") || response.status == 400 {
        Error::invalid(message)
    } else if response.status == 403 {
        Error::denied(message)
    } else if response.status == 404 {
        Error::not_found(message)
    } else {
        Error::unavailable(message)
    }
}

/// RFC 3339 `YYYY-MM-DDTHH:MM:SS[.fff]Z` or a numeric `+hh:mm` offset.
pub fn parse_timestamp(value: &str) -> Option<u64> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let bytes = value.as_bytes();
    let number =
        |start: usize, end: usize| -> Option<i64> { value.get(start..end)?.parse::<i64>().ok() };
    let year = number(0, 4)?;
    let month = number(5, 7)?;
    let day = number(8, 10)?;
    let hour = number(11, 13)?;
    let minute = number(14, 16)?;
    let second = number(17, 19)?;
    if bytes.len() < 19 {
        return None;
    }
    let mut offset = 0i64;
    if let Some(rest) = value.get(19..) {
        let rest = rest.trim_start_matches('.');
        let rest = if rest.starts_with(|character: char| character.is_ascii_digit()) {
            let end = rest
                .find(|character: char| !character.is_ascii_digit())
                .unwrap_or(rest.len());
            &rest[end..]
        } else {
            rest
        };
        if rest == "Z" || rest.is_empty() {
        } else if let Some(sign) = rest.chars().next().filter(|c| *c == '+' || *c == '-') {
            let hours = rest.get(1..3)?.parse::<i64>().ok()?;
            let minutes = rest.get(4..6)?.parse::<i64>().ok()?;
            offset = (hours * 3600 + minutes * 60) * if sign == '-' { -1 } else { 1 };
        }
    }
    let days = days_from_civil(year, month, day)?;
    Some((days * 86_400 + hour * 3600 + minute * 60 + second - offset).max(0) as u64)
}

fn days_from_civil(year: i64, month: i64, day: i64) -> Option<i64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let shifted = (month + 9) % 12;
    let day_of_year = (153 * shifted + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    Some(era * 146_097 + day_of_era - 719_468)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_parse_with_offsets() {
        assert_eq!(parse_timestamp("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_timestamp("2026-11-25T12:00:00Z"), Some(1_795_608_000));
        assert_eq!(
            parse_timestamp("2026-11-25T13:00:00+01:00"),
            Some(1_795_608_000)
        );
        assert_eq!(
            parse_timestamp("2026-11-25T12:00:00.123Z"),
            Some(1_795_608_000)
        );
        assert_eq!(parse_timestamp("nonsense"), None);
    }
}
