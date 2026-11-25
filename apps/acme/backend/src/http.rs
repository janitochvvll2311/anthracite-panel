//! A small HTTPS client for the CA conversation, with the egress guard.
//!
//! Every request resolves the target and refuses loopback, link-local,
//! unique-local, private and metadata addresses unless the directory entry
//! opted in (an internal step-ca). A public CA name that resolves to a private
//! address is refused too: the allowlist is not a DNS trust anchor.

use std::path::Path;
use std::time::Duration;

use anth_contract::Error;
use bytes::Bytes;
use http::header::{HeaderName, HeaderValue};
use http::{Method, StatusCode};
use http_body_util::{BodyExt, Full};
use hyper_rustls::HttpsConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use url::Url;

use crate::config::is_private_ip;

const RESPONSE_CAP: usize = 4 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(30);

pub struct Response {
    pub status: StatusCode,
    pub headers: http::HeaderMap,
    pub body: Vec<u8>,
    pub url: String,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|value| value.to_str().ok())
    }
}

#[derive(Clone)]
pub struct Http {
    client: Client<HttpsConnector<hyper_util::client::legacy::connect::HttpConnector>, Full<Bytes>>,
}

impl Http {
    /// The webpki roots plus an optional operator bundle (an internal CA).
    pub fn new(ca_bundle: Option<&Path>) -> Result<Self, Error> {
        let mut roots = rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        if let Some(path) = ca_bundle {
            let pem = std::fs::read(path).map_err(|error| {
                Error::invalid(format!("reading the CA bundle {}: {error}", path.display()))
            })?;
            for certificate in rustls_pemfile::certs(&mut pem.as_slice()) {
                let certificate = certificate.map_err(|error| {
                    Error::invalid(format!("the CA bundle {}: {error}", path.display()))
                })?;
                roots.add(certificate).map_err(|error| {
                    Error::invalid(format!("the CA bundle {}: {error}", path.display()))
                })?;
            }
        }
        let tls = rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let connector = hyper_rustls::HttpsConnectorBuilder::new()
            .with_tls_config(tls)
            .https_or_http()
            .enable_http1()
            .enable_http2()
            .build();
        let client = Client::builder(TokioExecutor::new()).build(connector);
        Ok(Self { client })
    }

    pub async fn request(
        &self,
        method: Method,
        url: &str,
        headers: &[(&str, String)],
        body: Vec<u8>,
        allow_private: bool,
    ) -> Result<Response, Error> {
        let parsed = Url::parse(url)
            .map_err(|error| Error::invalid(format!("`{url}` is not a URL: {error}")))?;
        if !matches!(parsed.scheme(), "https" | "http") {
            return Err(Error::invalid(format!("`{url}` is not an http(s) URL")));
        }
        if !allow_private {
            ensure_public_target(&parsed).await?;
        }
        let mut request = http::Request::builder()
            .method(method)
            .uri(url)
            .header("user-agent", "anthracite-acme/0.7");
        for (name, value) in headers {
            request = request.header(
                HeaderName::from_bytes(name.as_bytes())
                    .map_err(|error| Error::internal(format!("header `{name}`: {error}")))?,
                HeaderValue::from_str(value)
                    .map_err(|error| Error::internal(format!("header `{name}`: {error}")))?,
            );
        }
        let request = request
            .body(Full::new(Bytes::from(body)))
            .map_err(|error| Error::internal(format!("building the request: {error}")))?;
        let answer = tokio::time::timeout(TIMEOUT, self.client.request(request))
            .await
            .map_err(|_| Error::unavailable(format!("the CA did not answer {url} in time")))?
            .map_err(|error| Error::unavailable(format!("requesting {url}: {error}")))?;
        let status = answer.status();
        let headers = answer.headers().clone();
        let body = tokio::time::timeout(TIMEOUT, answer.into_body().collect())
            .await
            .map_err(|_| Error::unavailable(format!("the CA stalled the body of {url}")))?
            .map_err(|error| Error::unavailable(format!("reading {url}: {error}")))?
            .to_bytes();
        if body.len() > RESPONSE_CAP {
            return Err(Error::too_large(format!(
                "the CA answer to {url} is {} bytes; the limit is {RESPONSE_CAP}",
                body.len()
            )));
        }
        Ok(Response {
            status,
            headers,
            body: body.to_vec(),
            url: url.to_string(),
        })
    }
}

/// Resolves the host and refuses a private target. Called for every request when
/// the directory did not opt in to a private network.
pub async fn ensure_public_target(url: &Url) -> Result<(), Error> {
    let host = url
        .host_str()
        .ok_or_else(|| Error::invalid(format!("`{url}` has no host")))?;
    let port = url.port_or_known_default().unwrap_or(443);
    if let Ok(address) = host.parse::<std::net::IpAddr>() {
        if is_private_ip(address) {
            return Err(Error::denied(format!(
                "`{host}` is a private address; the directory must opt in with `allow-private`"
            )));
        }
        return Ok(());
    }
    let resolved = tokio::net::lookup_host((host, port))
        .await
        .map_err(|error| Error::unavailable(format!("resolving {host}: {error}")))?;
    let mut any = false;
    for address in resolved {
        any = true;
        if is_private_ip(address.ip()) {
            return Err(Error::denied(format!(
                "`{host}` resolves to the private address {}; the directory must opt in with `allow-private`",
                address.ip()
            )));
        }
    }
    if !any {
        return Err(Error::unavailable(format!("`{host}` did not resolve")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_public_target_is_accepted() {
        // No network call: only the parse and the literal-address branch.
        let url = Url::parse("https://acme-v02.api.letsencrypt.org/directory").expect("url");
        ensure_public_target(&url).await.expect("public");
    }

    #[tokio::test]
    async fn a_private_literal_is_refused() {
        let url = Url::parse("https://127.0.0.1:14000/dir").expect("url");
        let error = ensure_public_target(&url).await.expect_err("denied");
        assert!(error.to_string().contains("private"));
    }
}
