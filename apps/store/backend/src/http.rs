//! A small HTTPS client for the catalogue with the private-address guard.

use std::time::Duration;

use anth_contract::Error;
use bytes::Bytes;
use http::header::HeaderName;
use http::{Method, StatusCode};
use http_body_util::{BodyExt, Full};
use hyper_rustls::HttpsConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use url::Url;

use crate::config::is_private_ip;

const TIMEOUT: Duration = Duration::from_secs(30);

pub struct Response {
    pub status: StatusCode,
    pub headers: http::HeaderMap,
    pub body: Vec<u8>,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|value| value.to_str().ok())
    }

    pub fn ok(&self) -> bool {
        self.status.is_success()
    }
}

#[derive(Clone)]
pub struct Http {
    client: Client<HttpsConnector<hyper_util::client::legacy::connect::HttpConnector>, Full<Bytes>>,
}

impl Http {
    pub fn new() -> Result<Self, Error> {
        let connector = hyper_rustls::HttpsConnectorBuilder::new()
            .with_webpki_roots()
            .https_or_http()
            .enable_http1()
            .enable_http2()
            .build();
        Ok(Self {
            client: Client::builder(TokioExecutor::new()).build(connector),
        })
    }

    pub async fn get(
        &self,
        url: &str,
        headers: &[(&str, String)],
        allow_private: bool,
        cap: usize,
    ) -> Result<Response, Error> {
        let parsed = Url::parse(url)
            .map_err(|error| Error::invalid(format!("`{url}` is not a URL: {error}")))?;
        if !matches!(parsed.scheme(), "https" | "http") {
            return Err(Error::invalid(format!("`{url}` is not http(s)")));
        }
        if !allow_private {
            ensure_public(&parsed).await?;
        }
        let mut request = http::Request::builder()
            .method(Method::GET)
            .uri(url)
            .header("user-agent", "anthracite-store/0.7");
        for (name, value) in headers {
            request = request.header(
                HeaderName::from_bytes(name.as_bytes())
                    .map_err(|error| Error::internal(format!("header `{name}`: {error}")))?,
                http::HeaderValue::from_str(value)
                    .map_err(|error| Error::internal(format!("header `{name}`: {error}")))?,
            );
        }
        let request = request
            .body(Full::new(Bytes::new()))
            .map_err(|error| Error::internal(format!("building the request: {error}")))?;
        let answer = tokio::time::timeout(TIMEOUT, self.client.request(request))
            .await
            .map_err(|_| Error::unavailable(format!("`{url}` did not answer in time")))?
            .map_err(|error| Error::unavailable(format!("requesting {url}: {error}")))?;
        let status = answer.status();
        let headers = answer.headers().clone();
        let body = tokio::time::timeout(TIMEOUT, answer.into_body().collect())
            .await
            .map_err(|_| Error::unavailable(format!("`{url}` stalled its body")))?
            .map_err(|error| Error::unavailable(format!("reading {url}: {error}")))?
            .to_bytes();
        if body.len() > cap {
            return Err(Error::too_large(format!(
                "`{url}` answered {} bytes; the limit is {cap}",
                body.len()
            )));
        }
        Ok(Response {
            status,
            headers,
            body: body.to_vec(),
        })
    }
}

async fn ensure_public(url: &Url) -> Result<(), Error> {
    let host = url
        .host_str()
        .ok_or_else(|| Error::invalid(format!("`{url}` has no host")))?;
    let port = url.port_or_known_default().unwrap_or(443);
    if let Ok(address) = host.parse::<std::net::IpAddr>() {
        if is_private_ip(address) {
            return Err(Error::denied(format!(
                "`{host}` is a private address; the source must set allow-private"
            )));
        }
        return Ok(());
    }
    let resolved = tokio::net::lookup_host((host, port))
        .await
        .map_err(|error| Error::unavailable(format!("resolving {host}: {error}")))?;
    for address in resolved {
        if is_private_ip(address.ip()) {
            return Err(Error::denied(format!(
                "`{host}` resolves to a private address; the source must set allow-private"
            )));
        }
    }
    Ok(())
}
