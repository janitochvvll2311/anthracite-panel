//! The data plane: real listeners, vhost and route matching, upstream
//! forwarding, TLS termination and the WebSocket tunnel.
//!
//! The listeners are the two binds frozen in the manifest argv. The document
//! chooses which of them are active and how hosts map onto them; it cannot open
//! a port the operator did not approve. A bind that is not in the document
//! answers `503` instead of guessing.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use bytes::Bytes;
use http::header::{
    CONNECTION, CONTENT_LENGTH, CONTENT_TYPE, HOST, LOCATION, STRICT_TRANSPORT_SECURITY,
    TRANSFER_ENCODING, UPGRADE,
};
use http::{HeaderName, HeaderValue, Method, Request, Response, StatusCode, Version};
use http_body_util::combinators::UnsyncBoxBody;
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto;
use tokio::io::copy_bidirectional;
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

use crate::config::{match_route, Header, Matcher};
use crate::logs::AccessEvent;
use crate::service::{now, HostPlan, Service};

pub type BoxError = Box<dyn std::error::Error + Send + Sync>;
/// The response body the proxy can build either from bytes or from an upstream
/// stream. `Unsync` is enough: hyper polls it on the connection task.
pub type ProxyBody = UnsyncBoxBody<Bytes, BoxError>;

const ACME_PREFIX: &str = "/.well-known/acme-challenge/";
const HSTS: &str = "max-age=31536000";
/// A client that opens TCP and never finishes the handshake must not hold a
/// permit forever.
const TLS_HANDSHAKE: std::time::Duration = std::time::Duration::from_secs(10);

/// A body with no error case, boxed.
pub fn empty() -> ProxyBody {
    Full::new(Bytes::new())
        .map_err(|never| match never {})
        .boxed_unsync()
}

pub fn full(bytes: impl Into<Bytes>) -> ProxyBody {
    Full::new(bytes.into())
        .map_err(|never| match never {})
        .boxed_unsync()
}

fn text(status: StatusCode, message: &str) -> Response<ProxyBody> {
    let body = full(message.to_string());
    Response::builder()
        .status(status)
        .header(CONTENT_TYPE, "text/plain; charset=utf-8")
        .header(CONTENT_LENGTH, message.len())
        .body(body)
        .expect("a static error response")
}

/// Binds the two manifest endpoints. A failed bind is reported in `status` and
/// the other keeps serving (design §9, port conflicts).
pub async fn start(service: Arc<Service>) {
    for (bind, tls) in [
        (service.endpoints.http.clone(), false),
        (service.endpoints.https.clone(), true),
    ] {
        let listener = match TcpListener::bind(socket_address(&bind)).await {
            Ok(listener) => listener,
            Err(error) => {
                eprintln!("proxy: {bind}: {error}");
                service.note_error(&format!("{bind}: {error}"));
                continue;
            }
        };
        let address = match listener.local_addr() {
            Ok(address) => address,
            Err(error) => {
                service.note_error(&format!("{bind}: {error}"));
                continue;
            }
        };
        service.record_bound(&bind, address.to_string());
        let service = Arc::clone(&service);
        tokio::spawn(async move { accept(service, bind, tls, listener).await });
    }
}

async fn accept(service: Arc<Service>, bind: String, tls: bool, listener: TcpListener) {
    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(pair) => pair,
            Err(error) => {
                eprintln!("proxy: accepting on {bind}: {error}");
                continue;
            }
        };
        let permit = match service.permits.clone().try_acquire_owned() {
            Ok(permit) => permit,
            Err(_) => continue,
        };
        let service = Arc::clone(&service);
        let bind = bind.clone();
        tokio::spawn(async move {
            let _permit = permit;
            if tls {
                let config = service.snapshot().tls.clone();
                let Some(config) = config else {
                    return;
                };
                let accepted =
                    tokio::time::timeout(TLS_HANDSHAKE, TlsAcceptor::from(config).accept(stream))
                        .await;
                if let Ok(Ok(stream)) = accepted {
                    serve(service, bind, peer, true, stream).await;
                }
            } else {
                serve(service, bind, peer, false, stream).await;
            }
        });
    }
}

async fn serve<I>(service: Arc<Service>, bind: String, peer: SocketAddr, tls: bool, io: I)
where
    I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let builder = auto::Builder::new(TokioExecutor::new());
    let connection = builder.serve_connection_with_upgrades(
        TokioIo::new(io),
        service_fn(move |request: Request<Incoming>| {
            let service = Arc::clone(&service);
            let bind = bind.clone();
            async move { Ok::<_, Infallible>(handle(service, bind, peer, tls, request).await) }
        }),
    );
    let _ = connection.await;
}

/// `:8080` is the manifest form; std wants a host for the bind.
fn socket_address(bind: &str) -> String {
    if bind.starts_with(':') {
        format!("0.0.0.0{bind}")
    } else {
        bind.to_string()
    }
}

async fn handle(
    service: Arc<Service>,
    bind: String,
    peer: SocketAddr,
    tls: bool,
    request: Request<Incoming>,
) -> Response<ProxyBody> {
    let started = Instant::now();
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let snapshot = service.snapshot();
    let plan = &snapshot.plan;

    let host_header = request
        .headers()
        .get(HOST)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
        .or_else(|| {
            request
                .uri()
                .authority()
                .map(|authority| authority.to_string())
        });
    let selected = plan.select_host(&bind, host_header.as_deref());
    let (listener, host, host_name) = match selected {
        Some(selected) => selected,
        None => {
            let response = text(StatusCode::MISDIRECTED_REQUEST, "unknown host");
            log(
                &service,
                &host_name_of(host_header.as_deref()),
                &method,
                &path,
                &response,
                0,
                started,
                "",
            );
            return response;
        }
    };

    if method == Method::CONNECT {
        let response = text(StatusCode::METHOD_NOT_ALLOWED, "CONNECT is not proxied");
        log(
            &service, &host_name, &method, &path, &response, 0, started, "",
        );
        return response;
    }
    if request.version() <= Version::HTTP_11 && request.uri().scheme().is_some() {
        let response = text(
            StatusCode::BAD_REQUEST,
            "absolute-form requests are not proxied",
        );
        log(
            &service, &host_name, &method, &path, &response, 0, started, "",
        );
        return response;
    }

    // ACME is served before redirects and routes, and is not a route to an
    // upstream (design §6).
    if host.acme && path.starts_with(ACME_PREFIX) {
        let response = acme(&service.acme_root, &path).await;
        log(
            &service, &host_name, &method, &path, &response, 0, started, "",
        );
        return response;
    }

    if !listener.tls && host.redirect_https && host.has_certificate {
        if let Some(port) = plan.https_port {
            // The header may be an unknown name served by the default host; a
            // redirect must not echo it or it becomes an open redirect.
            let target = if host.names.iter().any(|name| name == &host_name) {
                host_name.clone()
            } else {
                host.names.first().cloned().unwrap_or_default()
            };
            let location = redirect_target(&target, port, &request);
            let response = Response::builder()
                .status(StatusCode::PERMANENT_REDIRECT)
                .header(LOCATION, location)
                .body(empty())
                .expect("a redirect response");
            log(
                &service, &host_name, &method, &path, &response, 0, started, "",
            );
            return response;
        }
    }

    let Some(route) = match_route(&host.routes, &path) else {
        let response = text(StatusCode::NOT_FOUND, "no route for this path");
        log(
            &service, &host_name, &method, &path, &response, 0, started, "",
        );
        return response;
    };

    if let Some(target) = &route.redirect {
        let location = if target.starts_with('/') {
            format!("{target}{}", query_suffix(&request))
        } else {
            target.clone()
        };
        let response = Response::builder()
            .status(StatusCode::FOUND)
            .header(LOCATION, location)
            .body(empty())
            .expect("a redirect response");
        log(
            &service, &host_name, &method, &path, &response, 0, started, "",
        );
        return response;
    }

    let upstream_id = route.upstream.clone().unwrap_or_default();
    let upgrade = is_websocket_upgrade(&request);
    if upgrade && !route.websocket {
        let response = text(
            StatusCode::UPGRADE_REQUIRED,
            "this route does not accept WebSocket upgrades",
        );
        log(
            &service,
            &host_name,
            &method,
            &path,
            &response,
            0,
            started,
            &upstream_id,
        );
        return response;
    }
    if !route.websocket && request.headers().contains_key(UPGRADE) && !upgrade {
        let response = text(StatusCode::BAD_REQUEST, "unsupported upgrade");
        log(
            &service,
            &host_name,
            &method,
            &path,
            &response,
            0,
            started,
            &upstream_id,
        );
        return response;
    }

    let response = forward(
        &snapshot,
        listener,
        host,
        route,
        &upstream_id,
        &host_name,
        peer,
        tls,
        upgrade,
        request,
    )
    .await;
    let bytes = content_length(&response);
    log(
        &service,
        &host_name,
        &method,
        &path,
        &response,
        bytes,
        started,
        &upstream_id,
    );
    response
}

#[allow(clippy::too_many_arguments)]
async fn forward(
    snapshot: &crate::service::Snapshot,
    listener: &crate::service::ListenerPlan,
    host: &HostPlan,
    route: &crate::config::Route,
    upstream_id: &str,
    host_name: &str,
    peer: SocketAddr,
    tls: bool,
    upgrade: bool,
    mut request: Request<Incoming>,
) -> Response<ProxyBody> {
    let Some(upstream) = snapshot.config.upstream(upstream_id) else {
        return text(StatusCode::BAD_GATEWAY, "the upstream is not configured");
    };
    let Some(client) = snapshot.clients.get(upstream_id) else {
        return text(StatusCode::BAD_GATEWAY, "the upstream client is missing");
    };
    let (scheme, authority) = match crate::config::parse_upstream_url(&upstream.url) {
        Ok(parts) => parts,
        Err(message) => return text(StatusCode::BAD_GATEWAY, &message),
    };

    let original_path = request.uri().path();
    let path = match route.matcher() {
        Matcher::Prefix(prefix) if route.strip_prefix => {
            let stripped = original_path.strip_prefix(&prefix).unwrap_or(original_path);
            if stripped.is_empty() {
                "/".to_string()
            } else if stripped.starts_with('/') {
                stripped.to_string()
            } else {
                format!("/{stripped}")
            }
        }
        _ => original_path.to_string(),
    };
    let uri = format!(
        "{scheme}://{authority}{path}{}",
        request
            .uri()
            .query()
            .map(|query| format!("?{query}"))
            .unwrap_or_default()
    );
    let uri: http::Uri = match uri.parse() {
        Ok(uri) => uri,
        Err(_) => return text(StatusCode::BAD_GATEWAY, "the upstream URL is invalid"),
    };

    let downstream = if upgrade {
        Some(hyper::upgrade::on(&mut request))
    } else {
        None
    };
    let method = request.method().clone();

    let limit = route.body_limit();
    if let Some(length) = request
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
    {
        if length > limit {
            return text(StatusCode::PAYLOAD_TOO_LARGE, "the body is over the limit");
        }
    }

    let mut headers = http::HeaderMap::new();
    for (name, value) in request.headers() {
        if name == HOST || hop_by_hop(name.as_str()) {
            continue;
        }
        if !listener.forwarded && is_forwarded(name.as_str()) {
            continue;
        }
        headers.append(name.clone(), value.clone());
    }
    if upgrade {
        headers.insert(CONNECTION, HeaderValue::from_static("upgrade"));
        if let Some(upgrade_value) = request.headers().get(UPGRADE) {
            headers.insert(UPGRADE, upgrade_value.clone());
        }
    }
    if listener.forwarded {
        headers.insert(
            HeaderName::from_static("x-forwarded-for"),
            HeaderValue::from_str(&peer.ip().to_string())
                .unwrap_or_else(|_| HeaderValue::from_static("unknown")),
        );
        headers.insert(
            HeaderName::from_static("x-forwarded-proto"),
            HeaderValue::from_static(if tls { "https" } else { "http" }),
        );
        if let Ok(value) = HeaderValue::from_str(host_name) {
            headers.insert(HeaderName::from_static("x-forwarded-host"), value);
        }
    }

    let body = Limited::new(request.into_body(), limit as usize)
        .map_err(BoxError::from)
        .boxed_unsync();
    let mut upstream_request = match Request::builder().method(method).uri(uri).body(body) {
        Ok(request) => request,
        Err(_) => return text(StatusCode::BAD_GATEWAY, "the upstream request is invalid"),
    };
    *upstream_request.headers_mut() = headers;

    let timeout = std::time::Duration::from_millis(upstream.read_timeout_ms as u64);
    let mut response = match tokio::time::timeout(timeout, client.request(upstream_request)).await {
        Ok(Ok(response)) => response,
        Ok(Err(error)) => {
            return text(
                StatusCode::BAD_GATEWAY,
                &format!("the upstream did not answer: {error}"),
            );
        }
        Err(_) => {
            return text(
                StatusCode::GATEWAY_TIMEOUT,
                "the upstream exceeded the read timeout",
            )
        }
    };

    if response.status() == StatusCode::SWITCHING_PROTOCOLS && upgrade {
        let upstream_upgrade = hyper::upgrade::on(&mut response);
        if let Some(downstream) = downstream {
            tokio::spawn(async move {
                let (downstream, upstream) = match (downstream.await, upstream_upgrade.await) {
                    (Ok(downstream), Ok(upstream)) => (downstream, upstream),
                    _ => return,
                };
                let mut downstream = TokioIo::new(downstream);
                let mut upstream = TokioIo::new(upstream);
                let _ = copy_bidirectional(&mut downstream, &mut upstream).await;
            });
        }
    }

    let (parts, body) = response.into_parts();
    let mut headers = response_headers(&parts.headers, &route.headers, host.hsts && tls);
    if parts.status == StatusCode::SWITCHING_PROTOCOLS {
        // A 101 must carry the upgrade back to the client; those two headers
        // are the only hop-by-hop ones that survive.
        if let Some(upgrade) = parts.headers.get(UPGRADE) {
            headers.insert(UPGRADE, upgrade.clone());
        }
        headers.insert(CONNECTION, HeaderValue::from_static("upgrade"));
    }
    let mut response = Response::new(body.map_err(BoxError::from).boxed_unsync());
    *response.status_mut() = parts.status;
    *response.headers_mut() = headers;
    response
}

fn is_websocket_upgrade(request: &Request<Incoming>) -> bool {
    request
        .headers()
        .get(UPGRADE)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.eq_ignore_ascii_case("websocket"))
        .unwrap_or(false)
        && request
            .headers()
            .get(CONNECTION)
            .and_then(|value| value.to_str().ok())
            .map(|value| value.to_ascii_lowercase().contains("upgrade"))
            .unwrap_or(false)
}

fn hop_by_hop(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}

fn is_forwarded(name: &str) -> bool {
    name.to_ascii_lowercase().starts_with("x-forwarded-")
}

fn query_suffix(request: &Request<Incoming>) -> String {
    request
        .uri()
        .query()
        .map(|query| format!("?{query}"))
        .unwrap_or_default()
}

fn redirect_target(host: &str, port: u16, request: &Request<Incoming>) -> String {
    let authority = if port == 443 {
        host.to_string()
    } else {
        format!("{host}:{port}")
    };
    format!(
        "https://{authority}{}{}",
        request.uri().path(),
        query_suffix(request)
    )
}

async fn acme(root: &Path, path: &str) -> Response<ProxyBody> {
    let token = &path[ACME_PREFIX.len()..];
    let safe = !token.is_empty()
        && token.len() <= 255
        && token.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        });
    if !safe {
        return text(StatusCode::NOT_FOUND, "not found");
    }
    match tokio::fs::read(root.join(token)).await {
        Ok(bytes) => Response::builder()
            .status(StatusCode::OK)
            .header(CONTENT_TYPE, "text/plain")
            .body(full(bytes))
            .expect("an acme response"),
        Err(_) => text(StatusCode::NOT_FOUND, "not found"),
    }
}

fn content_length(response: &Response<ProxyBody>) -> u64 {
    response
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok())
        .unwrap_or(0)
}

fn response_headers(upstream: &http::HeaderMap, route: &[Header], hsts: bool) -> http::HeaderMap {
    let mut headers = http::HeaderMap::new();
    for (name, value) in upstream {
        if hop_by_hop(name.as_str()) || name == TRANSFER_ENCODING {
            continue;
        }
        headers.append(name.clone(), value.clone());
    }
    for header in route {
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(header.name.as_bytes()),
            HeaderValue::from_str(&header.value),
        ) {
            headers.append(name, value);
        }
    }
    if hsts {
        headers.insert(STRICT_TRANSPORT_SECURITY, HeaderValue::from_static(HSTS));
    }
    headers
}

fn host_name_of(header: Option<&str>) -> String {
    header
        .map(crate::config::normalize_host_header)
        .unwrap_or_default()
}

#[allow(clippy::too_many_arguments)]
fn log(
    service: &Service,
    host: &str,
    method: &Method,
    path: &str,
    response: &Response<ProxyBody>,
    bytes: u64,
    started: Instant,
    upstream: &str,
) {
    service.record(AccessEvent {
        at: now(),
        host: host.to_string(),
        method: method.as_str().to_string(),
        path: path.to_string(),
        status: response.status().as_u16(),
        bytes,
        duration_ms: started.elapsed().as_millis() as u64,
        upstream: upstream.to_string(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::header::TE;

    #[test]
    fn hop_by_hop_headers_are_stripped() {
        for name in [
            "connection",
            "Keep-Alive",
            "Proxy-Authenticate",
            "TE",
            "Trailer",
            "Transfer-Encoding",
            "Upgrade",
        ] {
            assert!(hop_by_hop(name), "{name}");
        }
        assert!(!hop_by_hop("content-type"));
        assert!(!hop_by_hop("sec-websocket-key"));
    }

    #[test]
    fn forwarded_headers_are_recognized() {
        assert!(is_forwarded("X-Forwarded-For"));
        assert!(is_forwarded("x-forwarded-host"));
        assert!(!is_forwarded("x-request-id"));
    }

    #[test]
    fn a_bind_with_a_bare_port_gets_a_host() {
        assert_eq!(socket_address(":8080"), "0.0.0.0:8080");
        assert_eq!(socket_address("127.0.0.1:8443"), "127.0.0.1:8443");
    }

    #[test]
    fn response_headers_keep_cookies_and_drop_hop_by_hop() {
        let mut upstream = http::HeaderMap::new();
        upstream.insert(
            http::header::SET_COOKIE,
            HeaderValue::from_static("session=abc; HttpOnly"),
        );
        upstream.insert(TE, HeaderValue::from_static("trailers"));
        let route = vec![Header {
            name: "x-frame-options".to_string(),
            value: "DENY".to_string(),
        }];
        let headers = response_headers(&upstream, &route, true);
        assert!(headers.contains_key(http::header::SET_COOKIE));
        assert!(!headers.contains_key(TE));
        assert_eq!(
            headers
                .get("x-frame-options")
                .and_then(|value| value.to_str().ok()),
            Some("DENY")
        );
        assert_eq!(
            headers
                .get(STRICT_TRANSPORT_SECURITY)
                .and_then(|value| value.to_str().ok()),
            Some(HSTS)
        );
    }
}
