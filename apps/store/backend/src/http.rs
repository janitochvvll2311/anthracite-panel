//! A small HTTPS client for the catalogue with the private-address guard.
//!
//! The guard is enforced by a custom resolver, not only by a pre-flight check:
//! the connector can never connect to a private address even if DNS changes
//! between the check and the connection.

use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use anth_contract::Error;
use bytes::Bytes;
use http::header::HeaderName;
use http::{Method, StatusCode};
use http_body_util::{BodyExt, Full, Limited};
use hyper_rustls::HttpsConnector;
use hyper_util::client::legacy::connect::dns::{GaiResolver, Name};
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use tower_service::Service;
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
    public: Client<HttpsConnector<HttpConnector<PublicResolver>>, Full<Bytes>>,
    private: Client<HttpsConnector<HttpConnector>, Full<Bytes>>,
}

impl Http {
    pub fn new() -> Result<Self, Error> {
        let mut public_http = HttpConnector::new_with_resolver(PublicResolver::default());
        public_http.enforce_http(false);
        let public_connector = hyper_rustls::HttpsConnectorBuilder::new()
            .with_webpki_roots()
            .https_or_http()
            .enable_http1()
            .enable_http2()
            .wrap_connector(public_http);
        let private_connector = hyper_rustls::HttpsConnectorBuilder::new()
            .with_webpki_roots()
            .https_or_http()
            .enable_http1()
            .enable_http2()
            .build();
        Ok(Self {
            public: Client::builder(TokioExecutor::new()).build(public_connector),
            private: Client::builder(TokioExecutor::new()).build(private_connector),
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
        let answer = if allow_private {
            let answer = tokio::time::timeout(TIMEOUT, self.private.request(request))
                .await
                .map_err(|_| Error::unavailable(format!("`{url}` did not answer in time")))?
                .map_err(|error| Error::unavailable(format!("requesting {url}: {error}")))?;
            return read(answer, url, cap).await;
        } else {
            tokio::time::timeout(TIMEOUT, self.public.request(request))
                .await
                .map_err(|_| Error::unavailable(format!("`{url}` did not answer in time")))?
                .map_err(|error| Error::unavailable(format!("requesting {url}: {error}")))?
        };
        read(answer, url, cap).await
    }
}

async fn read(
    answer: http::Response<hyper::body::Incoming>,
    url: &str,
    cap: usize,
) -> Result<Response, Error> {
    let status = answer.status();
    let headers = answer.headers().clone();
    let body = tokio::time::timeout(TIMEOUT, Limited::new(answer.into_body(), cap).collect())
        .await
        .map_err(|_| Error::unavailable(format!("`{url}` stalled its body")))?
        .map_err(|error| Error::too_large(format!("`{url}`: {error}")))?
        .to_bytes();
    Ok(Response {
        status,
        headers,
        body: body.to_vec(),
    })
}

/// Resolves only public addresses; the connector never sees a private one.
#[derive(Clone)]
pub struct PublicResolver {
    inner: GaiResolver,
}

impl Default for PublicResolver {
    fn default() -> Self {
        Self {
            inner: GaiResolver::new(),
        }
    }
}

impl Service<Name> for PublicResolver {
    type Response = std::vec::IntoIter<SocketAddr>;
    type Error = io::Error;
    type Future =
        Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + 'static>>;

    fn poll_ready(&mut self, _context: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, name: Name) -> Self::Future {
        let mut inner = self.inner.clone();
        Box::pin(async move {
            let addresses = inner
                .call(name)
                .await
                .map_err(|error| io::Error::other(error.to_string()))?;
            let public: Vec<SocketAddr> = addresses
                .filter(|address| !is_private_ip(address.ip()))
                .collect();
            if public.is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "the source resolves only to private addresses",
                ));
            }
            Ok(public.into_iter())
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
