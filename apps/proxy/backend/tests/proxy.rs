//! End-to-end tests over real loopback sockets: two hosts, TLS/SNI/HTTP2,
//! WebSocket echo, apply/rollback and the access log.
//!
//! Every fixture binds `127.0.0.1:0` and reads the assigned port from the
//! service, so the tests never depend on a fixed port.

use std::convert::Infallible;
use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anth_contract::ErrorKind;
use anth_service::Handler;
use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use http::{Method, Request, Response, StatusCode, Version};
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::{TokioExecutor, TokioIo};
use rcgen::{BasicConstraints, Certificate, CertificateParams, DnType, IsCa, KeyPair};
use rustls::pki_types::{CertificateDer, ServerName};
use tokio::net::{TcpListener, TcpStream};

use proxy_service::admin::ADMIN;
use proxy_service::config::{Config, Endpoints, Header, Host, Listener, Route, Upstream};
use proxy_service::plane;
use proxy_service::service::Service;

type PlainClient = Client<HttpConnector, Full<Bytes>>;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("anthracite-panel")
        .join(format!("proxy-test-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("scratch");
    dir
}

fn endpoints() -> Endpoints {
    // Distinct strings: the service keys bound sockets by the configured bind,
    // and both ends are ephemeral here.
    Endpoints {
        http: "127.0.0.1:0".to_string(),
        https: "127.0.0.2:0".to_string(),
    }
}

fn upstream(id: &str, url: String) -> Upstream {
    Upstream {
        id: id.to_string(),
        url,
        connect_timeout_ms: 2_000,
        read_timeout_ms: 5_000,
        tls_verify: true,
    }
}

fn route(upstream: &str) -> Route {
    Route {
        id: "root".to_string(),
        r#match: "/".to_string(),
        upstream: Some(upstream.to_string()),
        strip_prefix: false,
        websocket: false,
        redirect: None,
        headers: Vec::new(),
        body_limit: None,
    }
}

fn host(id: &str, name: &str, upstream: &str, certificate: Option<&str>) -> Host {
    Host {
        id: id.to_string(),
        names: vec![name.to_string()],
        certificate: certificate.map(str::to_string),
        acme: false,
        redirect_https: false,
        hsts: false,
        routes: vec![route(upstream)],
    }
}

fn listener(bind: &str, tls: bool, default_host: Option<&str>) -> Listener {
    Listener {
        id: if tls { "secure" } else { "plain" }.to_string(),
        bind: bind.to_string(),
        tls,
        default_host: default_host.map(str::to_string),
        forwarded: false,
    }
}

struct Fixture {
    service: Arc<Service>,
    root: PathBuf,
    end: Endpoints,
}

impl Fixture {
    async fn start(name: &str, end: Endpoints, config: Config) -> Self {
        let root = scratch(name);
        let service = Service::new(
            root.join("data"),
            root.join("tls"),
            root.join("acme"),
            None,
            end.clone(),
        );
        service.apply(config, 0).expect("the first document");
        plane::start(Arc::clone(&service)).await;
        Self { service, root, end }
    }

    fn http(&self) -> SocketAddr {
        self.service
            .bound(&self.end.http)
            .expect("the http listener bound")
            .parse()
            .expect("an address")
    }

    fn status(&self) -> proxy_service::admin::ProxyStatus {
        self.service.status()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn plain_config(end: &Endpoints, hosts: Vec<Host>, upstreams: Vec<Upstream>) -> Config {
    Config {
        version: 0,
        listeners: vec![listener(&end.http, false, None)],
        upstreams,
        hosts,
    }
}

/// An upstream that answers `<name>:<path>`, so routing and rewriting are
/// visible in the body.
async fn text_upstream(name: &'static str) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("upstream");
    let address = listener.local_addr().expect("address");
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let service = service_fn(move |request: Request<Incoming>| async move {
                    let path = request
                        .uri()
                        .path_and_query()
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "/".to_string());
                    let body = format!("{name}:{path}");
                    Ok::<_, Infallible>(
                        Response::builder()
                            .status(StatusCode::OK)
                            .body(Full::new(Bytes::from(body)))
                            .expect("response"),
                    )
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), service)
                    .await;
            });
        }
    });
    address
}

async fn websocket_echo_upstream() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("ws upstream");
    let address = listener.local_addr().expect("address");
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let Ok(mut socket) = tokio_tungstenite::accept_async(stream).await else {
                    return;
                };
                while let Some(Ok(message)) = socket.next().await {
                    if message.is_text() || message.is_binary() {
                        let _ = socket.send(message).await;
                    }
                }
            });
        }
    });
    address
}

fn client() -> PlainClient {
    Client::builder(TokioExecutor::new()).build(HttpConnector::new())
}

async fn get(
    client: &PlainClient,
    address: SocketAddr,
    host: &str,
    path: &str,
) -> (StatusCode, Version, String) {
    let request = Request::builder()
        .uri(format!("http://{address}{path}"))
        .header("host", host)
        .body(Full::new(Bytes::new()))
        .expect("request");
    let response = client.request(request).await.expect("response");
    let status = response.status();
    let version = response.version();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    (status, version, String::from_utf8_lossy(&body).to_string())
}

// --- TLS fixtures ---------------------------------------------------------

struct Authority {
    certificate: Certificate,
    key: KeyPair,
}

fn authority() -> Authority {
    let mut params = CertificateParams::default();
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params
        .distinguished_name
        .push(DnType::CommonName, "anthracite proxy test ca");
    let key = KeyPair::generate().expect("ca key");
    let certificate = params.self_signed(&key).expect("ca certificate");
    Authority { certificate, key }
}

impl Authority {
    fn der(&self) -> CertificateDer<'static> {
        self.certificate.der().clone()
    }
}

struct Leaf {
    chain: String,
    key: String,
    der: CertificateDer<'static>,
}

fn leaf(authority: &Authority, name: &str) -> Leaf {
    let mut params = CertificateParams::new(vec![name.to_string()]).expect("params");
    params.distinguished_name.push(DnType::CommonName, name);
    let key = KeyPair::generate().expect("leaf key");
    let certificate = params
        .signed_by(&key, &authority.certificate, &authority.key)
        .expect("leaf certificate");
    Leaf {
        chain: certificate.pem(),
        key: key.serialize_pem(),
        der: certificate.der().clone(),
    }
}

fn publish(tls: &Path, id: &str, leaf: &Leaf) {
    let dir = tls.join(id);
    fs::create_dir_all(&dir).expect("certificate dir");
    fs::write(dir.join("fullchain.pem"), &leaf.chain).expect("chain");
    fs::write(dir.join("privkey.pem"), &leaf.key).expect("key");
}

fn tls_client(root: CertificateDer<'static>, alpn: &[&[u8]]) -> rustls::ClientConfig {
    let mut roots = rustls::RootCertStore::empty();
    roots.add(root).expect("root certificate");
    let mut config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .expect("versions")
    .with_root_certificates(roots)
    .with_no_client_auth();
    config.alpn_protocols = alpn.iter().map(|protocol| protocol.to_vec()).collect();
    config
}

async fn tls_connect(
    address: SocketAddr,
    name: &str,
    config: rustls::ClientConfig,
) -> tokio_rustls::client::TlsStream<TcpStream> {
    let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
    let tcp = TcpStream::connect(address).await.expect("tcp");
    connector
        .connect(
            ServerName::try_from(name.to_string()).expect("a server name"),
            tcp,
        )
        .await
        .expect("tls")
}

// --- Tests ----------------------------------------------------------------

#[tokio::test]
async fn two_hosts_route_to_two_upstreams() {
    let end = endpoints();
    let a = text_upstream("A").await;
    let b = text_upstream("B").await;
    let config = plain_config(
        &end,
        vec![
            host("a", "a.test", "a", None),
            host("b", "b.test", "b", None),
        ],
        vec![
            upstream("a", format!("http://{a}")),
            upstream("b", format!("http://{b}")),
        ],
    );
    let fixture = Fixture::start("two-hosts", end, config).await;
    let client = client();

    let (status, version, body) = get(&client, fixture.http(), "a.test", "/one").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(version, Version::HTTP_11);
    assert_eq!(body, "A:/one");

    let (status, _, body) = get(&client, fixture.http(), "B.TEST", "/two?x=1").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "B:/two?x=1");

    let (status, _, _) = get(&client, fixture.http(), "unknown.test", "/").await;
    assert_eq!(status, StatusCode::MISDIRECTED_REQUEST);
}

#[tokio::test]
async fn the_default_host_serves_an_unknown_name() {
    let end = endpoints();
    let a = text_upstream("A").await;
    let mut config = plain_config(
        &end,
        vec![host("a", "a.test", "a", None)],
        vec![upstream("a", format!("http://{a}"))],
    );
    config.listeners[0].default_host = Some("a".to_string());
    let fixture = Fixture::start("default-host", end, config).await;

    let (status, _, body) = get(&client(), fixture.http(), "whatever.test", "/x").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "A:/x");
}

#[tokio::test]
async fn exact_routes_beat_prefixes_through_the_proxy() {
    let end = endpoints();
    let a = text_upstream("A").await;
    let b = text_upstream("B").await;
    let mut site = host("a", "a.test", "a", None);
    site.routes = vec![
        Route {
            id: "health".to_string(),
            r#match: "= /health".to_string(),
            upstream: Some("b".to_string()),
            strip_prefix: false,
            websocket: false,
            redirect: None,
            headers: Vec::new(),
            body_limit: None,
        },
        route("a"),
    ];
    let config = plain_config(
        &end,
        vec![site],
        vec![
            upstream("a", format!("http://{a}")),
            upstream("b", format!("http://{b}")),
        ],
    );
    let fixture = Fixture::start("precedence", end, config).await;
    let client = client();

    assert_eq!(
        get(&client, fixture.http(), "a.test", "/health").await.2,
        "B:/health"
    );
    assert_eq!(
        get(&client, fixture.http(), "a.test", "/health/deep")
            .await
            .2,
        "A:/health/deep"
    );
}

#[tokio::test]
async fn absolute_form_requests_are_refused() {
    let end = endpoints();
    let a = text_upstream("A").await;
    let config = plain_config(
        &end,
        vec![host("a", "a.test", "a", None)],
        vec![upstream("a", format!("http://{a}"))],
    );
    let fixture = Fixture::start("absolute-form", end, config).await;

    let stream = TcpStream::connect(fixture.http()).await.expect("tcp");
    let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .expect("handshake");
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let request = Request::builder()
        .method(Method::GET)
        .uri("http://a.test/x")
        .header("host", "a.test")
        .body(Full::new(Bytes::new()))
        .expect("request");
    let response = sender.send_request(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn apply_rolls_back_and_guards_the_version() {
    let end = endpoints();
    let a = text_upstream("A").await;
    let b = text_upstream("B").await;
    let good = plain_config(
        &end,
        vec![host("a", "a.test", "a", None)],
        vec![upstream("a", format!("http://{a}"))],
    );
    let fixture = Fixture::start("rollback", end.clone(), good.clone()).await;
    let client = client();
    assert_eq!(get(&client, fixture.http(), "a.test", "/").await.2, "A:/");
    assert_eq!(fixture.status().version, 1);

    let mut bad = good.clone();
    bad.hosts[0].routes[0].upstream = Some("missing".to_string());
    let error = fixture.service.apply(bad, 1).expect_err("refused");
    assert_eq!(error.kind(), ErrorKind::Invalid);
    assert_eq!(fixture.status().version, 1);
    assert_eq!(
        get(&client, fixture.http(), "a.test", "/").await.2,
        "A:/",
        "the last good document keeps serving"
    );

    let stale = fixture.service.apply(good.clone(), 0).expect_err("stale");
    assert_eq!(stale.kind(), ErrorKind::Conflict);

    let mut next = good.clone();
    next.hosts[0].routes[0].upstream = Some("b".to_string());
    next.upstreams.push(upstream("b", format!("http://{b}")));
    let applied = fixture.service.apply(next, 1).expect("applied");
    assert_eq!(applied.version, 2);
    assert_eq!(get(&client, fixture.http(), "a.test", "/").await.2, "B:/");

    let persisted = fs::read_to_string(fixture.root.join("data/config.json")).expect("config");
    let parsed: Config = serde_json::from_str(&persisted).expect("parsed config");
    assert_eq!(parsed.version, 2);
}

#[tokio::test]
async fn tls_serves_by_sni_and_negotiates_http2() {
    let end = endpoints();
    let authority = authority();
    let a = leaf(&authority, "a.test");
    let b = leaf(&authority, "b.test");
    let root = scratch("tls-sni");
    let tls_dir = root.join("tls");
    publish(&tls_dir, "a", &a);
    publish(&tls_dir, "b", &b);

    let up_a = text_upstream("A").await;
    let up_b = text_upstream("B").await;
    let config = Config {
        version: 0,
        listeners: vec![listener(&end.https, true, Some("a"))],
        upstreams: vec![
            upstream("a", format!("http://{up_a}")),
            upstream("b", format!("http://{up_b}")),
        ],
        hosts: vec![
            host("a", "a.test", "a", Some("a")),
            host("b", "b.test", "b", Some("b")),
        ],
    };
    let service = Service::new(
        root.join("data"),
        tls_dir,
        root.join("acme"),
        None,
        end.clone(),
    );
    service.apply(config, 0).expect("apply");
    plane::start(Arc::clone(&service)).await;
    let address: SocketAddr = service
        .bound(&end.https)
        .expect("bound")
        .parse()
        .expect("address");

    // HTTP/2 through ALPN.
    let config = tls_client(authority.der(), &[b"h2", b"http/1.1"]);
    let stream = tls_connect(address, "a.test", config).await;
    assert_eq!(
        stream.get_ref().1.alpn_protocol(),
        Some(b"h2".as_ref()),
        "h2 is negotiated"
    );
    let (mut sender, connection) =
        hyper::client::conn::http2::handshake(TokioExecutor::new(), TokioIo::new(stream))
            .await
            .expect("h2 handshake");
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let request = Request::builder()
        .uri("https://a.test/")
        .header("host", "a.test")
        .body(Full::new(Bytes::new()))
        .expect("request");
    let response = sender.send_request(request).await.expect("h2 response");
    assert_eq!(response.version(), Version::HTTP_2);
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    assert_eq!(&body[..], b"A:/");

    // A second name selects the other certificate and the other upstream.
    let config = tls_client(authority.der(), &[b"http/1.1"]);
    let stream = tls_connect(address, "b.test", config).await;
    let peer = stream
        .get_ref()
        .1
        .peer_certificates()
        .and_then(|certs| certs.first().cloned())
        .expect("peer certificate");
    assert_eq!(peer.as_ref(), b.der.as_ref(), "SNI selects b's certificate");
    let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .expect("h1 handshake");
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let request = Request::builder()
        .uri("/")
        .header("host", "b.test")
        .body(Full::new(Bytes::new()))
        .expect("request");
    let response = sender.send_request(request).await.expect("h1 response");
    assert_eq!(response.version(), Version::HTTP_11);
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    assert_eq!(&body[..], b"B:/");

    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn a_published_certificate_reloads_without_a_restart() {
    let end = endpoints();
    let authority = authority();
    let first = leaf(&authority, "a.test");
    let second = leaf(&authority, "a.test");
    let root = scratch("tls-reload");
    let tls_dir = root.join("tls");
    publish(&tls_dir, "a", &first);

    let up = text_upstream("A").await;
    let config = Config {
        version: 0,
        listeners: vec![listener(&end.https, true, Some("a"))],
        upstreams: vec![upstream("a", format!("http://{up}"))],
        hosts: vec![host("a", "a.test", "a", Some("a"))],
    };
    let service = Service::new(
        root.join("data"),
        tls_dir.clone(),
        root.join("acme"),
        None,
        end.clone(),
    );
    service.apply(config, 0).expect("apply");
    plane::start(Arc::clone(&service)).await;
    let address: SocketAddr = service
        .bound(&end.https)
        .expect("bound")
        .parse()
        .expect("address");

    let peer = |address: SocketAddr, root: CertificateDer<'static>| async move {
        let config = tls_client(root, &[b"http/1.1"]);
        let stream = tls_connect(address, "a.test", config).await;
        stream
            .get_ref()
            .1
            .peer_certificates()
            .and_then(|certs| certs.first().cloned())
            .expect("peer")
    };
    let before = peer(address, authority.der()).await;
    assert_eq!(before.as_ref(), first.der.as_ref());

    publish(&tls_dir, "a", &second);
    service.reload_certificates().expect("reload");
    let after = peer(address, authority.der()).await;
    assert_eq!(
        after.as_ref(),
        second.der.as_ref(),
        "the new file is served"
    );

    fs::write(tls_dir.join("a/fullchain.pem"), "not a certificate").expect("corrupt");
    assert!(
        service.reload_certificates().is_err(),
        "an invalid file is refused"
    );
    let kept = peer(address, authority.der()).await;
    assert_eq!(
        kept.as_ref(),
        second.der.as_ref(),
        "the last good resolver keeps serving"
    );
    assert!(service.status().last_error.is_some());

    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn a_websocket_echo_passes_through() {
    let end = endpoints();
    let echo = websocket_echo_upstream().await;
    let mut site = host("a", "a.test", "a", None);
    site.routes[0].websocket = true;
    let mut config = plain_config(
        &end,
        vec![site],
        vec![upstream("a", format!("http://{echo}"))],
    );
    // The test client connects to the loopback address, so the default host
    // must be the name the route is declared under.
    config.listeners[0].default_host = Some("a".to_string());
    let fixture = Fixture::start("websocket", end, config).await;

    let stream = TcpStream::connect(fixture.http()).await.expect("tcp");
    let (mut socket, response) =
        tokio_tungstenite::client_async(format!("ws://{}/chat", fixture.http()), stream)
            .await
            .expect("websocket");
    assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);
    socket
        .send(tokio_tungstenite::tungstenite::Message::Text(
            "hello".into(),
        ))
        .await
        .expect("send");
    let reply = tokio::time::timeout(Duration::from_secs(5), socket.next())
        .await
        .expect("echo timeout")
        .expect("an echoed frame")
        .expect("a message");
    assert_eq!(reply.to_text().expect("text"), "hello");
}

#[tokio::test]
async fn the_access_log_streams_one_event_per_request() {
    let end = endpoints();
    let a = text_upstream("A").await;
    let config = plain_config(
        &end,
        vec![host("a", "a.test", "a", None)],
        vec![upstream("a", format!("http://{a}"))],
    );
    let fixture = Fixture::start("access-log", end, config).await;
    let mut stream = fixture
        .service
        .stream(ADMIN, "logs", serde_json::Value::Null)
        .expect("log stream");

    let (status, _, _) = get(&client(), fixture.http(), "a.test", "/logged").await;
    assert_eq!(status, StatusCode::OK);

    let chunk = tokio::time::timeout(Duration::from_secs(5), stream.next())
        .await
        .expect("event timeout")
        .expect("an event")
        .expect("bytes");
    let event: serde_json::Value = serde_json::from_slice(&chunk).expect("json");
    assert_eq!(event["host"], "a.test");
    assert_eq!(event["path"], "/logged");
    assert_eq!(event["status"], 200);
    assert_eq!(event["upstream"], "a");
}

#[tokio::test]
async fn the_admin_surface_answers_the_document_and_the_cli_views() {
    let end = endpoints();
    let a = text_upstream("A").await;
    let config = plain_config(
        &end,
        vec![host("a", "a.test", "a", None)],
        vec![upstream("a", format!("http://{a}"))],
    );
    let fixture = Fixture::start("admin", end, config).await;
    let service = Arc::clone(&fixture.service);

    let snapshot = service
        .call(ADMIN, "snapshot", serde_json::json!({}))
        .expect("snapshot");
    assert_eq!(snapshot["version"], 1);
    assert_eq!(snapshot["hosts"][0]["names"][0], "a.test");

    let hosts = service
        .call(ADMIN, "hosts", serde_json::json!({}))
        .expect("hosts");
    assert_eq!(hosts.as_array().map(Vec::len), Some(1));

    let routes = service
        .call(ADMIN, "routes", serde_json::json!({}))
        .expect("routes");
    assert_eq!(routes[0]["match"], "/");

    let certificates = service
        .call(ADMIN, "certificates", serde_json::json!({}))
        .expect("certificates");
    assert_eq!(certificates.as_array().map(Vec::len), Some(0));

    let wrong = service.call(ADMIN, "nope", serde_json::json!({}));
    assert_eq!(wrong.expect_err("not found").kind(), ErrorKind::NotFound);

    let probe =
        tokio::task::spawn_blocking(move || service.test_upstream_blocking("a").expect("probe"))
            .await
            .expect("join");
    assert!(probe.ok, "{probe:?}");
    assert_eq!(probe.status, Some(200));
}

#[tokio::test]
async fn a_host_with_a_certificate_redirects_plain_http() {
    let end = endpoints();
    let authority = authority();
    let cert = leaf(&authority, "a.test");
    let root = scratch("redirect");
    let tls_dir = root.join("tls");
    publish(&tls_dir, "a", &cert);
    let up = text_upstream("A").await;

    let mut site = host("a", "a.test", "a", Some("a"));
    site.redirect_https = true;
    let config = Config {
        version: 0,
        listeners: vec![
            listener(&end.http, false, Some("a")),
            listener(&end.https, true, Some("a")),
        ],
        upstreams: vec![upstream("a", format!("http://{up}"))],
        hosts: vec![site],
    };
    let service = Service::new(
        root.join("data"),
        tls_dir,
        root.join("acme"),
        None,
        end.clone(),
    );
    service.apply(config, 0).expect("apply");
    plane::start(Arc::clone(&service)).await;
    let address: SocketAddr = service
        .bound(&end.http)
        .expect("bound")
        .parse()
        .expect("address");

    let request = Request::builder()
        .uri(format!("http://{address}/x?y=1"))
        .header("host", "a.test")
        .body(Full::new(Bytes::new()))
        .expect("request");
    let response = client().request(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::PERMANENT_REDIRECT);
    let location = response
        .headers()
        .get("location")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    assert!(location.starts_with("https://a.test"), "{location}");
    assert!(location.ends_with("/x?y=1"), "{location}");

    // An unknown Host is served by the default host, but the redirect must use
    // the document's name, never echo the header (open redirect).
    let forged = Request::builder()
        .uri(format!("http://{address}/x"))
        .header("host", "evil.example")
        .body(Full::new(Bytes::new()))
        .expect("request");
    let response = client().request(forged).await.expect("response");
    assert_eq!(response.status(), StatusCode::PERMANENT_REDIRECT);
    let location = response
        .headers()
        .get("location")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    assert!(location.starts_with("https://a.test"), "{location}");
    assert!(!location.contains("evil.example"), "{location}");

    let _ = fs::remove_dir_all(root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_applies_have_one_winner() {
    let end = endpoints();
    let a = text_upstream("A").await;
    let base = plain_config(
        &end,
        vec![host("a", "a.test", "a", None)],
        vec![upstream("a", format!("http://{a}"))],
    );
    let fixture = Fixture::start("apply-race", end, base).await;
    let service = Arc::clone(&fixture.service);
    let current = service.snapshot().config.clone();
    assert_eq!(current.version, 1);

    let mut handles = Vec::new();
    for index in 0..2 {
        let service = Arc::clone(&service);
        let mut document = current.clone();
        document.hosts[0].routes[0].headers = vec![Header {
            name: "x-attempt".to_string(),
            value: index.to_string(),
        }];
        handles.push(tokio::task::spawn_blocking(move || {
            service.apply(document, 1)
        }));
    }
    let mut applied = 0;
    let mut conflicts = 0;
    for handle in handles {
        match handle.await.expect("join") {
            Ok(_) => applied += 1,
            Err(error) => {
                assert_eq!(error.kind(), ErrorKind::Conflict, "{error:?}");
                conflicts += 1;
            }
        }
    }
    assert_eq!((applied, conflicts), (1, 1));
    assert_eq!(service.status().version, 2);
}
