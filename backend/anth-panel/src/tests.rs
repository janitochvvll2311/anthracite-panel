//! Tests for the modules at the root of this binary.
//!
//! One file per level: the HTTP router, the dispatcher, the CLI, the table and
//! the socket are siblings in `src/`, so their tests are siblings here. The
//! `plugins/` subtree keeps its own `tests.rs`, one per level.

/// A logger that writes nothing, for tests about something else.
///
/// Shared because these are crate-wide tests of the dispatcher, the router and
/// the socket: none of them asserts on a log line, and a test that did would
/// build its own sink to read.
pub(crate) fn quiet() -> std::sync::Arc<crate::log::Logger> {
    std::sync::Arc::new(crate::log::Logger::silent())
}

/// A router with a live session attached to every request.
///
/// The daemon varies per test but the session does not, so this is the one place
/// that wires the two together: register an account on a scratch file, log in,
/// and attach the token. A test that wants to see a `401` calls `crate::api::router`
/// directly instead of coming through here.
pub(crate) fn signed_router(daemon: crate::dispatch::Daemon, name: &str) -> axum::Router {
    let auth = std::sync::Arc::new(crate::auth::guard::Auth::load(scratch(name), true));
    auth.register("operator", "correct horse battery")
        .expect("registration must succeed on a fresh file");
    let token = auth
        .login("operator", "correct horse battery")
        .expect("the password just registered must verify")
        .access_token;
    let value = axum::http::HeaderValue::from_str(&format!("Bearer {token}"))
        .expect("a token is a valid header value");
    crate::api::router(std::sync::Arc::new(daemon), auth).layer(axum::middleware::from_fn(
        move |mut request: axum::extract::Request, next: axum::middleware::Next| {
            let value = value.clone();
            async move {
                request
                    .headers_mut()
                    .entry(axum::http::header::AUTHORIZATION)
                    .or_insert(value);
                next.run(request).await
            }
        },
    ))
}

/// A settings file for one test, under the scratch directory.
///
/// Named after the test so two tests cannot share an account, and placed under
/// `TMPDIR` like every other fixture, so a bug here cannot reach the system temp
/// directory.
pub(crate) fn scratch(name: &str) -> std::path::PathBuf {
    // A counter, because `test_router()` builds a fresh state per call and two
    // calls in one test would otherwise share one account — and a test that
    // registers twice would fail on the second, which says nothing about the
    // code it was written for.
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "anth-auth-{name}-{}-{sequence}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).expect("a scratch directory");
    directory.join("config.toml")
}

/// A snapshot with every field filled, for tests that care about the shape of an
/// answer rather than what this machine happens to report.
pub(crate) fn blank_host_info() -> anth_wire::HostInfo {
    anth_wire::HostInfo {
        hostname: "test-host".to_string(),
        os: anth_wire::OsInfo {
            name: "Test OS".to_string(),
            version: "1".to_string(),
            kernel: "6.0".to_string(),
            arch: "x86_64".to_string(),
        },
        uptime_seconds: 1,
        cpu_model: "Test CPU".to_string(),
        logical_cores: 2,
        load: anth_wire::LoadAverage {
            one: 0.0,
            five: 0.0,
            fifteen: 0.0,
        },
        memory: anth_wire::MemoryInfo {
            total_kib: 1024,
            available_kib: 512,
            swap_total_kib: 0,
            swap_free_kib: 0,
        },
    }
}

/// The HTTP transport, which must answer exactly what the socket does.
pub(crate) mod api {
    use crate::api::{ApiError, parse_query, router};
    use crate::dispatch::Daemon;
    use anth_wire::WireError;
    use axum::Router;
    use axum::body::Body;
    use axum::http::Request as HttpRequest;
    use axum::response::IntoResponse;
    use serde_json::json;
    use std::sync::Arc;
    use tower::ServiceExt;

    /// An auth state with no account, rooted in a scratch file.
    ///
    /// Every router now needs one. The bootstrap is open, so a test that is not
    /// about authentication reads and writes as if the guard were not there —
    /// and it is: `Authed` still runs, it just finds an account that exists.
    pub(crate) fn test_auth(name: &str) -> Arc<crate::auth::guard::Auth> {
        Arc::new(crate::auth::guard::Auth::load(
            crate::tests::scratch(name),
            true,
        ))
    }

    /// A router whose every request carries a valid session.
    ///
    /// The routes are protected now, so a test about a plugin, a file or the
    /// shell needs a token or it would only ever assert on `401`. This builds
    /// the account, logs in, and returns the pair a caller sends.
    pub(crate) fn authenticated(name: &str) -> (Router, String, Arc<crate::auth::guard::Auth>) {
        let auth = test_auth(name);
        auth.register("operator", "correct horse battery")
            .expect("registration must succeed on a fresh file");
        let token = login_token(&auth);
        let router = router(
            Arc::new(Daemon::for_test(
                crate::plugins::registry::Registry::empty(),
                crate::tests::quiet(),
            )),
            Arc::clone(&auth),
        );
        (router, token, auth)
    }

    /// An access token for an already-registered auth state.
    pub(crate) fn login_token(auth: &crate::auth::guard::Auth) -> String {
        let grant = auth
            .login("operator", "correct horse battery")
            .expect("the password just registered must verify");
        grant.access_token
    }

    /// A router every request of which carries a session.
    ///
    /// The layer is the test's stand-in for a browser: it attaches the
    /// `Authorization` header that `EventSource` and `fetch` would carry as a
    /// cookie. It exists so the tests written before authentication keep testing
    /// what they were written for — a route, not a credential — instead of all
    /// of them turning into `401` assertions.
    ///
    /// A test *about* authentication calls [`unauthenticated_router`], which is
    /// the same router without the layer.
    pub(crate) fn test_router() -> Router {
        authenticated_router("test-router")
    }

    pub(crate) fn authenticated_router(name: &str) -> Router {
        let (router, token, _auth) = authenticated(name);
        let value = axum::http::HeaderValue::from_str(&format!("Bearer {token}"))
            .expect("a token is a valid header value");
        router.layer(axum::middleware::from_fn(
            move |mut request: axum::extract::Request, next: axum::middleware::Next| {
                let value = value.clone();
                async move {
                    request
                        .headers_mut()
                        .entry(axum::http::header::AUTHORIZATION)
                        .or_insert(value);
                    next.run(request).await
                }
            },
        ))
    }

    /// A router with no session attached, and an auth state with no account.
    ///
    /// Two states in one value: `registered` is what a test flips by calling
    /// `auth.register`, and the router is the one that answers from it. Returned
    /// together because a test about the bootstrap needs to change the state and
    /// then ask the same router.
    pub(crate) fn unauthenticated_router(name: &str) -> (Router, Arc<crate::auth::guard::Auth>) {
        let auth = test_auth(name);
        let router = router(
            Arc::new(Daemon::for_test(
                crate::plugins::registry::Registry::empty(),
                crate::tests::quiet(),
            )),
            Arc::clone(&auth),
        );
        (router, auth)
    }

    /// A router with one disabled plugin, which is enough to prove a lookup.
    fn aliased_router() -> Router {
        crate::tests::signed_router(
            Daemon::for_test(
                crate::plugins::registry::Registry::with_disabled(
                    "org.test.aliased",
                    Some("short"),
                    &[("GET", "/greeting")],
                    "the bundle was rewritten",
                ),
                crate::tests::quiet(),
            ),
            "aliased",
        )
    }

    async fn get_status_and_body(uri: &str) -> (u16, serde_json::Value) {
        ask(test_router(), uri).await
    }

    pub(crate) async fn ask(router: Router, uri: &str) -> (u16, serde_json::Value) {
        let response = router
            .oneshot(
                HttpRequest::builder()
                    .uri(uri)
                    .body(Body::empty())
                    .expect("a valid request"),
            )
            .await
            .expect("the router must answer");

        let status = response.status().as_u16();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .expect("a readable body");
        let body = serde_json::from_slice(&bytes).expect("the body must be JSON");
        (status, body)
    }

    #[tokio::test]
    async fn healthz_answers_ok_without_touching_the_daemon() {
        let (status, body) = get_status_and_body("/healthz").await;
        assert_eq!(status, 200);
        assert_eq!(body["status"], json!("ok"));
    }

    #[tokio::test]
    async fn host_info_answers_the_same_snapshot_the_socket_returns() {
        let (status, body) = get_status_and_body("/api/host-info").await;

        assert_eq!(status, 200);
        // Read from the running container, so assert shape rather than values.
        assert!(
            body["hostname"].as_str().is_some_and(|s| !s.is_empty()),
            "{body}"
        );
        assert!(
            body["logical_cores"].as_u64().is_some_and(|n| n > 0),
            "{body}"
        );
        assert!(body["os"]["name"].is_string(), "{body}");
    }

    #[test]
    fn a_query_string_becomes_the_map_the_http_entry_expects() {
        let query = parse_query(Some("x=1&y=two&flag"));

        assert_eq!(query["x"], json!("1"));
        assert_eq!(query["y"], json!("two"));
        assert_eq!(query["flag"], json!(""));
    }

    #[test]
    fn an_absent_query_is_an_empty_map() {
        assert!(parse_query(None).is_empty());
        assert!(parse_query(Some("")).is_empty());
    }

    /// `/p/` is the web namespace and nothing else, so a path there that names
    /// no plugin is a `404` about the route's own shape — not "no anchor",
    /// which would describe a namespace `/p/` no longer serves. It must not fall
    /// through to the SPA either, which would answer `200 index.html`.
    #[tokio::test]
    async fn a_path_under_p_that_names_no_plugin_says_so() {
        let (status, body) = get_status_and_body("/p/nope").await;

        assert_eq!(status, 404);
        let error = body["error"].as_str().unwrap();
        assert!(error.contains("no plugin"), "{error}");
        assert!(error.contains("/p/<id-or-alias>/<module>"), "{error}");
        assert!(!error.contains("no anchor"), "{error}");
    }

    /// An anchor is an operation, so it lives under `/api/plugins/{id}/`. Asking
    /// `/p/` for one must not reach it: that is the whole point of the split.
    #[tokio::test]
    async fn an_anchor_is_not_reachable_under_the_web_namespace() {
        let (status, body) = ask(aliased_router(), "/p/greeting").await;

        // The fixture's only plugin is disabled, so a reachable anchor would
        // answer 403 naming it. A `404` about the plugin half of `/p/<id>/<m>`
        // is what proves the anchor route was never consulted.
        assert_eq!(status, 404, "body: {body}");
        let error = body["error"].as_str().unwrap();
        assert!(error.contains("no plugin"), "{error}");
    }

    /// The browser builds `<prefix>/<id><path>`, so the id-scoped route has to
    /// exist or every widget's first request is a 404.
    #[tokio::test]
    async fn an_unknown_plugin_anchor_is_a_404_that_names_the_plugin() {
        let (status, body) = get_status_and_body("/api/plugins/org.anth.absent/greeting").await;

        assert_eq!(status, 404);
        let error = body["error"].as_str().unwrap();
        assert!(error.contains("no anchor"), "{error}");
        assert!(error.contains("org.anth.absent"), "{error}");
    }

    /// `{id}` accepts the alias, so a widget built with the short handle reaches
    /// the same plugin. The disabled entry refuses on its *canonical* id, which
    /// is what proves the alias resolved rather than being echoed back.
    #[tokio::test]
    async fn a_scoped_anchor_accepts_the_plugin_alias() {
        let (status, body) = ask(aliased_router(), "/api/plugins/short/greeting").await;

        assert_eq!(status, 403);
        let error = body["error"].as_str().unwrap();
        assert!(error.contains("org.test.aliased"), "{error}");
    }

    /// The call route resolves the same way, because both go through the daemon.
    #[tokio::test]
    async fn a_plugin_call_accepts_the_plugin_alias() {
        let response = aliased_router()
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/api/plugins/short/call")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"payload":"greet"}"#))
                    .expect("a valid request"),
            )
            .await
            .expect("the router must answer");

        assert_eq!(response.status().as_u16(), 403);
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .expect("a readable body");
        let body: serde_json::Value =
            serde_json::from_slice(&bytes).expect("the body must be JSON");
        assert!(
            body["error"].as_str().unwrap().contains("org.test.aliased"),
            "{}",
            body["error"]
        );
    }

    #[tokio::test]
    async fn an_unknown_route_is_not_the_spa_fallback() {
        let response = test_router()
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/nope")
                    .body(Body::empty())
                    .expect("a valid request"),
            )
            .await
            .expect("the router must answer");

        // An unmatched path under /api must not be answered by the static
        // fallback with 200 and index.html.
        assert_eq!(response.status().as_u16(), 404);
    }

    /// A router whose readable root is a real directory with one file.
    fn readable_router(name: &str, contents: &str) -> (std::path::PathBuf, Router) {
        let root = std::env::temp_dir().join(format!("anth-api-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("the fixture root must be creatable");
        std::fs::write(root.join("log.txt"), contents).expect("the fixture must be written");
        let router = crate::tests::signed_router(
            Daemon::new(
                crate::plugins::registry::Registry::empty(),
                crate::tests::quiet(),
                &root,
            ),
            name,
        );
        (root, router)
    }

    /// The HTTP route carries the wait, and says when it waited.
    ///
    /// The wait is the one field a follower depends on, and this checks it
    /// crosses the transport rather than only existing on the wire type.
    #[tokio::test]
    async fn a_follow_over_http_says_when_it_waited_and_found_nothing() {
        let (root, router) = readable_router("follow", "done\n");

        let (status, body) = ask(
            router,
            "/api/files?path=log.txt&offset=5&len=64&wait_ms=100",
        )
        .await;

        assert_eq!(status, 200);
        assert_eq!(body["waited"], json!(true), "the wait must be reported");
        assert_eq!(body["eof"], json!(true));
        assert_eq!(body["bytes"], json!(""));
        assert_eq!(body["next"], json!(5), "the cursor must not move");
        std::fs::remove_dir_all(&root).expect("the fixture must be removable");
    }

    /// A wait above the daemon's cap is refused, not trimmed.
    ///
    /// Trimming would let a client believe it had a deadline it does not have,
    /// which is the silent shortening rule 1 forbids.
    #[tokio::test]
    async fn a_wait_above_the_cap_is_refused_over_http() {
        let (root, router) = readable_router("cap", "x");

        let (status, body) = ask(
            router,
            &format!(
                "/api/files?path=log.txt&wait_ms={}",
                crate::files::MAX_WAIT_MS + 1
            ),
        )
        .await;

        assert_eq!(status, 400, "an unusable wait is the caller's mistake");
        assert!(
            body["error"]
                .as_str()
                .is_some_and(|m| m.contains("wait_ms")),
            "the message must name the field: {body}"
        );
        std::fs::remove_dir_all(&root).expect("the fixture must be removable");
    }

    #[test]
    fn an_error_renders_its_mapped_status_and_a_flat_message() {
        use anth_wire::ErrorKind;

        let error = ApiError(WireError::new(
            ErrorKind::Forbidden,
            "control is off".to_string(),
        ));
        let response = error.into_response();

        assert_eq!(response.status().as_u16(), 403);
    }
}

/// The single request match both transports call.
mod dispatch {
    use crate::dispatch::{Daemon, encode_payload};
    use crate::plugins::registry::Registry;
    use anth_wire::{ErrorKind, PluginPipe, Request, Response};

    fn daemon_without_plugins() -> Daemon {
        Daemon::for_test(Registry::empty(), crate::tests::quiet())
    }

    #[tokio::test]
    async fn an_empty_registry_answers_a_list_with_nothing_in_it() {
        let response = daemon_without_plugins().handle(Request::PluginList).await;

        match response {
            Response::PluginList { plugins } => assert!(plugins.is_empty()),
            other => panic!("expected a list, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn calling_an_unknown_plugin_is_a_404() {
        let response = daemon_without_plugins()
            .handle(Request::PluginCall {
                id: "org.nobody".to_string(),
                pipe: PluginPipe::Cli,
                payload: "greet".to_string(),
            })
            .await;

        let error = response.error().expect("must be an error");
        assert_eq!(error.kind, ErrorKind::NotFound);
        assert!(error.message.contains("org.nobody"));
    }

    /// The alias is a way to *name* a plugin, not a second identity: a call that
    /// used it must land on the same plugin, and the refusal it produces names
    /// the canonical id so a caller can tell which one it reached.
    #[tokio::test]
    async fn a_call_by_alias_reaches_the_plugin_that_declared_it() {
        let daemon = Daemon::for_test(
            Registry::with_disabled(
                "org.test.aliased",
                Some("short"),
                &[],
                "the bundle was rewritten",
            ),
            crate::tests::quiet(),
        );

        let response = daemon
            .handle(Request::PluginCall {
                id: "short".to_string(),
                pipe: PluginPipe::Cli,
                payload: "greet".to_string(),
            })
            .await;

        let error = response.error().expect("a disabled plugin must refuse");
        assert_eq!(error.kind, ErrorKind::Forbidden);
        assert!(
            error.message.contains("org.test.aliased"),
            "the canonical id must be named, got: {}",
            error.message
        );
    }

    /// An alias resolves a *plugin*, so an anchor lookup scoped to it must find
    /// the same one its id would.
    #[tokio::test]
    async fn an_anchor_lookup_accepts_the_alias() {
        let daemon = Daemon::for_test(
            Registry::with_disabled(
                "org.test.aliased",
                Some("short"),
                &[("GET", "/greeting")],
                "disabled",
            ),
            crate::tests::quiet(),
        );

        // Both forms must reach the same plugin. This entry is disabled, so both
        // report the same refusal naming the canonical id: an alias is a way to
        // name a plugin, not a separate lookup path with its own rules.
        let by_alias = daemon.anchor_for("short", "GET", "/greeting").unwrap_err();
        let by_id = daemon
            .anchor_for("org.test.aliased", "GET", "/greeting")
            .unwrap_err();
        assert_eq!(by_alias.kind, by_id.kind);
        assert_eq!(by_alias.message, by_id.message);
        assert!(
            by_alias.message.contains("org.test.aliased"),
            "{}",
            by_alias.message
        );

        // A path the plugin does not declare is still `None` rather than a
        // refusal, so the alias does not change which paths a plugin owns.
        assert!(
            daemon
                .anchor_for("short", "GET", "/other")
                .unwrap()
                .is_none()
        );
    }

    /// An alias is discoverable from the list, or nobody would know to type it.
    #[tokio::test]
    async fn the_list_reports_the_alias() {
        let daemon = Daemon::for_test(
            Registry::with_disabled("org.test.aliased", Some("short"), &[], "disabled"),
            crate::tests::quiet(),
        );

        match daemon.handle(Request::PluginList).await {
            Response::PluginList { plugins } => {
                assert_eq!(plugins.len(), 1);
                assert_eq!(plugins[0].alias.as_deref(), Some("short"));
            }
            other => panic!("expected a list, got {other:?}"),
        }
    }

    /// Every pipe names its own ABI entry. A wrong mapping would silently send a
    /// command line down the HTTP entry, which is the confusion the three
    /// entries exist to prevent.
    /// The CLI entry parses a JSON string, so a bare line would be a `400` on
    /// every call. The host owns the encoding — the caller types a line.
    #[test]
    fn a_cli_payload_is_encoded_as_a_json_string() {
        let encoded = encode_payload(anth_wire::PluginPipe::Cli, r#"greet --name "Ana Sol""#);

        let decoded: String = serde_json::from_str(&encoded).expect("must be a JSON string");
        assert_eq!(decoded, r#"greet --name "Ana Sol""#);
    }

    /// The other two pipes already carry JSON, so re-encoding would double it.
    #[test]
    fn an_http_payload_passes_through_untouched() {
        let body = r#"{"message":"hi"}"#;

        assert_eq!(encode_payload(anth_wire::PluginPipe::Http, body), body);
        assert_eq!(encode_payload(anth_wire::PluginPipe::Direct, body), body);
    }

    #[test]
    fn each_pipe_names_its_own_entry() {
        assert_eq!(PluginPipe::Cli.entry(), "anth_cli");
        assert_eq!(PluginPipe::Http.entry(), "anth_http");
        assert_eq!(PluginPipe::Direct.entry(), "anth_direct");
    }
}

/// CLI parsing, and the exit code each outcome produces.
mod main {
    use crate::{Cli, Command, PluginAction, Startup, bind, startup};
    use anth_wire::PluginPipe;
    use clap::Parser;
    use std::process::ExitCode;

    #[test]
    fn the_cli_parses_the_host_info_subcommand() {
        let cli = Cli::try_parse_from(["anth-panel", "host-info"]).expect("parsing");
        assert!(matches!(
            cli.command,
            Some(Command::HostInfo { watch: None })
        ));
        assert!(cli.socket.is_none());
    }

    #[test]
    fn the_socket_flag_is_global_so_it_works_after_the_subcommand() {
        let cli = Cli::try_parse_from(["anth-panel", "host-info", "--socket", "/tmp/x.sock"])
            .expect("parsing");
        assert_eq!(cli.socket.as_deref(), Some("/tmp/x.sock"));
    }

    #[test]
    fn an_unknown_subcommand_is_refused() {
        assert!(Cli::try_parse_from(["anth-panel", "nonsense"]).is_err());
    }

    /// `--interactive` alone is valid, which is why `command` is optional.
    #[test]
    fn interactive_alone_needs_no_subcommand() {
        let cli = Cli::try_parse_from(["anth-panel", "-i"]).expect("parsing");
        assert!(cli.interactive);
        assert!(cli.command.is_none());
    }

    #[test]
    fn a_bare_invocation_parses_and_is_refused_by_main() {
        // Parsing must succeed so `main` can print a message instead of clap's
        // usage dump, which says nothing about `--interactive`.
        let cli = Cli::try_parse_from(["anth-panel"]).expect("parsing");
        assert!(cli.command.is_none());
        assert!(!cli.interactive);
    }

    /// `plugin install` is not a plugin call, so it has no pipe to stand in for.
    ///
    /// The daemon never sees it — installing is local — so the arm had to answer
    /// `None` rather than pick one, which would have made a filesystem action
    /// look like something the socket could ask for.
    #[test]
    fn an_install_is_not_a_pipe() {
        let cli = Cli::try_parse_from([
            "anth-panel",
            "plugin",
            "install",
            "plugins/hello/dist/org.anth.hello.zip",
        ])
        .expect("parsing");

        let Some(Command::Plugin { action }) = cli.command else {
            panic!("expected a plugin command");
        };
        assert_eq!(action.pipe(), None);
        assert!(!action.force());
    }

    #[test]
    fn install_force_replaces_and_defaults_to_refusing() {
        let plain =
            Cli::try_parse_from(["anth-panel", "plugin", "install", "a.zip"]).expect("parsing");
        let forced = Cli::try_parse_from(["anth-panel", "plugin", "install", "a.zip", "--force"])
            .expect("parsing");

        let bundle = |cli: Cli| match cli.command {
            Some(Command::Plugin {
                action: PluginAction::Install { bundle, force },
            }) => (bundle, force),
            other => panic!("expected an install, got {other:?}"),
        };
        assert_eq!(bundle(plain), ("a.zip".to_string(), false));
        assert_eq!(bundle(forced), ("a.zip".to_string(), true));
    }

    #[test]
    fn a_plugin_call_defaults_to_the_cli_pipe() {
        let cli = Cli::try_parse_from([
            "anth-panel",
            "plugin",
            "call",
            "org.anth.hello",
            r#"{"action":"greeting","value":null}"#,
        ])
        .expect("parsing");

        let Some(Command::Plugin { action }) = cli.command else {
            panic!("expected a plugin command");
        };
        assert_eq!(action.pipe(), Some(PluginPipe::Cli));
    }

    #[test]
    fn the_direct_flag_selects_the_direct_pipe() {
        let cli = Cli::try_parse_from([
            "anth-panel",
            "plugin",
            "call",
            "org.anth.hello",
            r#"{"action":"greeting","value":null}"#,
            "--direct",
        ])
        .expect("parsing");

        let Some(Command::Plugin { action }) = cli.command else {
            panic!("expected a plugin command");
        };
        assert_eq!(action.pipe(), Some(PluginPipe::Direct));
    }

    #[test]
    fn the_http_flag_selects_the_http_pipe() {
        let cli = Cli::try_parse_from([
            "anth-panel",
            "plugin",
            "call",
            "org.anth.hello",
            "{}",
            "--http",
        ])
        .expect("parsing");

        let Some(Command::Plugin { action }) = cli.command else {
            panic!("expected a plugin command");
        };
        assert_eq!(action.pipe(), Some(PluginPipe::Http));
    }

    /// The plugin owns its flags, so the panel must not claim them. Without
    /// `--` clap tried to parse `--name` itself and exited 2, which made the
    /// documented `plugin call <id> <line>` form unusable for any plugin that
    /// takes an option. `--` is the standard way to say where the panel's
    /// arguments stop; the panel's own flags must still work after the payload.
    #[test]
    fn a_plugin_flag_after_the_separator_reaches_the_plugin() {
        let cli = Cli::try_parse_from([
            "anth-panel",
            "plugin",
            "call",
            "org.anth.hello",
            "--",
            "greet",
            "--name",
            "Ana",
        ])
        .expect("parsing");

        let Some(Command::Plugin { action }) = cli.command else {
            panic!("expected a plugin command");
        };
        let PluginAction::Call { line, .. } = &action else {
            panic!("expected a call");
        };
        assert_eq!(
            crate::payload(line, action.pipe().expect("a call has a pipe")),
            "greet --name Ana"
        );
    }

    /// A word with a space in it arrives as one argument because the shell
    /// quoted it. Rejoining with a bare space would hand the plugin two words
    /// and the name would be torn in half, so the payload has to quote it back.
    #[test]
    fn a_quoted_word_survives_the_rejoin() {
        let cli = Cli::try_parse_from([
            "anth-panel",
            "plugin",
            "call",
            "org.anth.hello",
            "--",
            "greet",
            "--name",
            "Ana Sol",
        ])
        .expect("parsing");

        let Some(Command::Plugin { action }) = cli.command else {
            panic!("expected a plugin command");
        };
        let PluginAction::Call { line, .. } = &action else {
            panic!("expected a call");
        };

        // The panel's own splitter is the oracle: it must read the name back as
        // the single word the caller typed.
        assert_eq!(
            crate::payload(line, action.pipe().expect("a call has a pipe")),
            "greet --name 'Ana Sol'"
        );
        assert_eq!(
            crate::repl::split(&crate::payload(
                line,
                action.pipe().expect("a call has a pipe")
            )),
            vec!["greet", "--name", "Ana Sol"]
        );
    }

    /// A single argument is already a line: the caller quoted it, so re-quoting
    /// would make the plugin see one giant word and no subcommand. This is the
    /// form `plugin call <id> "<line>"` has always accepted and it must not
    /// regress when the argv form gains quoting.
    #[test]
    fn a_pre_quoted_line_is_passed_through_untouched() {
        let cli = Cli::try_parse_from([
            "anth-panel",
            "plugin",
            "call",
            "org.anth.hello",
            "greet --name 'Ana Sol'",
        ])
        .expect("parsing");

        let Some(Command::Plugin { action }) = cli.command else {
            panic!("expected a plugin command");
        };
        let PluginAction::Call { line, .. } = &action else {
            panic!("expected a call");
        };
        assert_eq!(
            crate::payload(line, action.pipe().expect("a call has a pipe")),
            "greet --name 'Ana Sol'"
        );
        assert_eq!(
            crate::repl::split(&crate::payload(
                line,
                action.pipe().expect("a call has a pipe")
            )),
            vec!["greet", "--name", "Ana Sol"]
        );
    }

    /// The HTTP and direct pipes take a JSON body the host never parses, so the
    /// words are joined verbatim: quoting there would corrupt the JSON.
    #[test]
    fn a_json_payload_is_joined_but_not_quoted() {
        let words = vec!["{\"who\":\"Ana Sol\"}".to_string()];
        assert_eq!(
            crate::payload(&words, PluginPipe::Http),
            "{\"who\":\"Ana Sol\"}"
        );
        assert_eq!(
            crate::payload(&words, PluginPipe::Direct),
            "{\"who\":\"Ana Sol\"}"
        );
    }

    #[tokio::test]
    async fn binding_a_regular_file_exits_two_because_the_caller_asked_wrong() {
        // Pins the wiring from ErrorKind to the process exit code. The daemon
        // path must derive its code from the kind instead of a literal `1`.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path =
            std::env::temp_dir().join(format!("anth-exit-{}-{nanos}.file", std::process::id()));
        std::fs::write(&path, b"not a socket").expect("writing the file");

        let error = bind(&path, None).await.err().expect("must refuse");
        assert_eq!(ExitCode::from(error.kind.exit_code()), ExitCode::from(2));
        assert_eq!(error.kind, anth_wire::ErrorKind::Invalid);

        std::fs::remove_file(&path).expect("removing the file");
    }

    /// `run -i` and `daemon -i` must serve *and* keep the prompt. Dispatching
    /// them instead started a loop that never returns, so the prompt was
    /// unreachable and `-i` was silently ignored.
    #[test]
    fn a_server_command_under_interactive_serves_behind_the_prompt() {
        let run = startup(Some(Command::Run {
            addr: "127.0.0.1:0".into(),
        }));
        let Startup::Serve { addr } = run else {
            panic!("`run -i` must serve, not run the daemon and block");
        };
        assert_eq!(addr.as_deref(), Some("127.0.0.1:0"));

        let daemon = startup(Some(Command::Daemon));
        let Startup::Serve { addr } = daemon else {
            panic!("`daemon -i` must serve, not block");
        };
        assert!(addr.is_none());
    }

    /// A client command still runs first, so `-i host-info` prints a snapshot
    /// and then keeps the prompt.
    #[test]
    fn a_client_command_under_interactive_runs_first_and_keeps_the_prompt() {
        let Startup::RunFirst(command) = startup(Some(Command::HostInfo { watch: None })) else {
            panic!("a client command must run before the prompt");
        };
        assert!(matches!(*command, Command::HostInfo { watch: None }));
    }

    #[test]
    fn interactive_alone_starts_nothing_but_the_prompt() {
        assert!(matches!(startup(None), Startup::Prompt));
    }
}

/// The bordered table `host-info` and `plugin list` print.
mod render {
    use crate::render::{format_kib, format_uptime, render_host_info, render_plugin_list};
    use crate::table::width;
    use anth_wire::{HostInfo, LoadAverage, MemoryInfo, OsInfo, PluginSummary};

    fn info() -> HostInfo {
        HostInfo {
            hostname: "anth".into(),
            os: OsInfo {
                name: "Alpine Linux".into(),
                version: "3.20".into(),
                kernel: "6.6.0".into(),
                arch: "x86_64".into(),
            },
            uptime_seconds: 90_061,
            cpu_model: "Example CPU".into(),
            logical_cores: 4,
            load: LoadAverage {
                one: 0.5,
                five: 0.25,
                fifteen: 0.125,
            },
            memory: MemoryInfo {
                total_kib: 2_048,
                available_kib: 1_024,
                swap_total_kib: 0,
                swap_free_kib: 0,
            },
        }
    }

    #[test]
    fn uptime_keeps_only_its_non_zero_components() {
        assert_eq!(format_uptime(0), "0s");
        assert_eq!(format_uptime(59), "59s");
        assert_eq!(format_uptime(3_600), "1h");
        assert_eq!(format_uptime(90_061), "1d 1h 1m 1s");
    }

    #[test]
    fn kib_uses_the_largest_readable_unit() {
        assert_eq!(format_kib(0), "0 KiB");
        assert_eq!(format_kib(512), "512 KiB");
        assert_eq!(format_kib(2_048), "2.0 MiB");
        assert_eq!(format_kib(8_388_608), "8.0 GiB");
    }

    #[test]
    fn the_table_is_a_closed_box_with_one_row_per_field() {
        let rendered = render_host_info(&info());
        let lines: Vec<&str> = rendered.lines().collect();

        assert!(lines[0].starts_with('┌') && lines[0].ends_with('┐'));
        assert!(lines.last().unwrap().starts_with('└'));
        assert!(lines.last().unwrap().ends_with('┘'));
        // A rule and a row alternate inside the box, so every interior line is
        // closed by one of the two vertical junctions.
        for line in &lines[1..lines.len() - 1] {
            assert!(
                (line.starts_with('│') || line.starts_with('├'))
                    && (line.ends_with('│') || line.ends_with('┤')),
                "an unclosed row: {line}"
            );
        }
        // Eleven fields, each with its own separating rule, plus the two ends.
        assert_eq!(lines.len(), 11 * 2 + 1, "eleven fields plus their rules");
    }

    #[test]
    fn every_line_has_the_same_character_width() {
        let rendered = render_host_info(&info());
        let widths: Vec<usize> = rendered.lines().map(width).collect();
        assert!(
            widths.windows(2).all(|w| w[0] == w[1]),
            "ragged table: {widths:?}"
        );
    }

    #[test]
    fn multi_byte_values_do_not_shift_the_border() {
        let mut snapshot = info();
        snapshot.hostname = "anfitrión".into();
        snapshot.cpu_model = "处理器".into();
        let rendered = render_host_info(&snapshot);
        let widths: Vec<usize> = rendered.lines().map(width).collect();
        assert!(
            widths.windows(2).all(|w| w[0] == w[1]),
            "multi-byte text broke the alignment: {widths:?}"
        );
    }

    #[test]
    fn the_values_appear_in_the_table() {
        let rendered = render_host_info(&info());
        assert!(rendered.contains("Alpine Linux"));
        assert!(rendered.contains("1d 1h 1m 1s"));
        assert!(rendered.contains("1.0 MiB used of 2.0 MiB"));
        assert!(rendered.contains("0 KiB used of 0 KiB"));
    }

    /// The alias is an accepted name, so a list that hid it would leave a caller
    /// guessing which short handles exist. A plugin without one leaves the
    /// column blank rather than shifting the row.
    #[test]
    fn the_plugin_list_shows_the_alias() {
        let rendered = render_plugin_list(&[
            PluginSummary {
                id: "org.test.aliased".into(),
                alias: Some("short".into()),
                name: "Aliased".into(),
                version: "0.2.0".into(),
                anchors: vec!["GET /greeting".into()],
                ui: vec![],
                disabled_reason: None,
            },
            PluginSummary {
                id: "org.test.plain".into(),
                alias: None,
                name: "Plain".into(),
                version: "0.2.0".into(),
                anchors: vec![],
                ui: vec![],
                disabled_reason: Some("the bundle was rewritten".into()),
            },
        ]);

        assert!(rendered.contains("ALIAS"));
        assert!(rendered.contains("short"));
        // The second plugin has no alias, so its cell is blank: the state and
        // anchors still line up under their headers.
        assert!(rendered.contains("disabled"));
        assert!(rendered.contains("the bundle was rewritten"));
        let lines: Vec<&str> = rendered.lines().collect();
        assert!(lines[0].starts_with('┌'), "the table must be bordered");
        assert!(lines[1].contains("ID"), "the header row comes first");
        assert!(lines.iter().any(|line| line.contains("Plain")));
    }

    #[test]
    fn an_empty_plugin_list_says_so() {
        assert_eq!(render_plugin_list(&[]), "no plugins installed");
    }
}

/// The border geometry every table in the CLI shares.
mod table {
    use crate::table::{Table, width};

    /// Every corner, tee and cross the border draws, in one grid: two header
    /// columns and two data rows, so all nine junctions appear.
    #[test]
    fn the_border_closes_every_corner_and_crosses_every_junction() {
        let rendered = Table::new()
            .with_headers(["A", "B"])
            .rows([["a", "b"], ["c", "d"]])
            .separate_rows(true)
            .render();

        assert_eq!(
            rendered.lines().collect::<Vec<_>>(),
            [
                "\u{250c}\u{2500}\u{2500}\u{2500}\u{252c}\u{2500}\u{2500}\u{2500}\u{2510}",
                "\u{2502} A \u{2502} B \u{2502}",
                "\u{251c}\u{2500}\u{2500}\u{2500}\u{253c}\u{2500}\u{2500}\u{2500}\u{2524}",
                "\u{2502} a \u{2502} b \u{2502}",
                "\u{251c}\u{2500}\u{2500}\u{2500}\u{253c}\u{2500}\u{2500}\u{2500}\u{2524}",
                "\u{2502} c \u{2502} d \u{2502}",
                "\u{2514}\u{2500}\u{2500}\u{2500}\u{2534}\u{2500}\u{2500}\u{2500}\u{2518}",
            ],
            "got:\n{rendered}"
        );
    }

    /// Without `separate_rows` there is no rule between data rows, but the
    /// header rule stays: it is what marks the header as a header.
    #[test]
    fn a_header_rule_does_not_imply_a_rule_between_data_rows() {
        let rendered = Table::new()
            .with_headers(["A"])
            .rows([["a"], ["b"]])
            .render();

        assert_eq!(
            rendered.lines().collect::<Vec<_>>(),
            [
                "\u{250c}\u{2500}\u{2500}\u{2500}\u{2510}",
                "\u{2502} A \u{2502}",
                "\u{251c}\u{2500}\u{2500}\u{2500}\u{2524}",
                "\u{2502} a \u{2502}",
                "\u{2502} b \u{2502}",
                "\u{2514}\u{2500}\u{2500}\u{2500}\u{2518}",
            ],
            "got:\n{rendered}"
        );
    }

    /// A short row is padded instead of shifting the right edge: the widest row
    /// decides the column count, so a missing cell cannot collapse a column.
    #[test]
    fn a_row_with_fewer_cells_is_padded_to_the_widest_row() {
        let rendered = Table::new().rows([vec!["a", "b"], vec!["c"]]).render();

        let lines: Vec<&str> = rendered.lines().collect();
        assert_eq!(lines.len(), 4, "top, two rows, bottom:\n{rendered}");
        assert_eq!(lines[2], "\u{2502} c \u{2502}   \u{2502}");
    }

    /// An empty table renders nothing rather than a zero-width box, which is
    /// what a caller that already checked for emptiness expects.
    #[test]
    fn a_table_with_no_rows_renders_nothing() {
        assert_eq!(Table::new().render(), "");
        assert_eq!(Table::new().separate_rows(true).render(), "");
    }

    /// The border is measured in characters, so a multi-byte cell does not
    /// narrow the box and a wide glyph still counts as one.
    #[test]
    fn a_multi_byte_cell_keeps_the_column_width() {
        let rendered = Table::new().rows([["ñ"], ["nn"]]).render();
        let widths: Vec<usize> = rendered.lines().map(width).collect();

        assert!(
            widths.windows(2).all(|w| w[0] == w[1]),
            "multi-byte text broke the alignment: {widths:?}"
        );
    }
}

/// The Unix socket, over a real listener in the test temp directory.
/// A socket path unique to this test, removed on drop.
///
/// Shared by the socket and watch suites: both need a real daemon on a real
/// listener, and a second copy of this helper would be a second place for the
/// naming to collide.
pub(crate) struct SocketPath(pub(crate) std::path::PathBuf);

impl SocketPath {
    pub(crate) fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        Self(std::env::temp_dir().join(format!("anth-{label}-{}-{nanos}.sock", std::process::id())))
    }
}

impl Drop for SocketPath {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Starts a daemon on a unique path and returns the path once it is bound.
pub(crate) async fn spawn_daemon(label: &str) -> SocketPath {
    let path = SocketPath::new(label);
    let listener = crate::socket::bind(&path.0).expect("binding the test socket");
    let daemon = std::sync::Arc::new(crate::dispatch::Daemon::for_test(
        crate::plugins::registry::Registry::empty(),
        crate::tests::quiet(),
    ));
    tokio::spawn(async move {
        let _ = crate::socket::serve(listener, daemon).await;
    });
    path
}

mod socket {
    use crate::socket::{Client, bind};
    use crate::tests::{SocketPath, spawn_daemon};
    use anth_wire::{Request, Response, read_frame, write_frame};
    use tokio::net::UnixStream;

    #[tokio::test]
    async fn a_request_reaches_the_daemon_over_the_socket() {
        let path = spawn_daemon("round-trip").await;
        let mut client = Client::connect(&path.0).await.expect("connecting");

        let response = client.call(&Request::HostInfo).await.expect("calling");

        match response {
            Response::HostInfo(info) => {
                assert!(!info.hostname.is_empty(), "the real host has a hostname");
                assert!(!info.os.arch.is_empty());
            }
            other => panic!("expected a snapshot, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn one_connection_can_carry_several_requests() {
        let path = spawn_daemon("pipeline").await;
        let mut client = Client::connect(&path.0).await.expect("connecting");

        let first = client
            .call(&Request::Hello {
                protocol_version: anth_wire::PROTOCOL_VERSION,
            })
            .await
            .expect("calling");
        assert!(matches!(first, Response::Welcome { .. }));

        let second = client.call(&Request::HostInfo).await.expect("calling");
        assert!(matches!(second, Response::HostInfo(_)));
    }

    #[tokio::test]
    async fn an_unreadable_request_is_answered_and_the_connection_survives() {
        let path = spawn_daemon("bad-request").await;

        let mut raw = UnixStream::connect(&path.0).await.expect("connecting");
        write_frame(&mut raw, b"{\"type\":\"nonsense\"}")
            .await
            .expect("writing");
        let reply = read_frame(&mut raw)
            .await
            .expect("reading")
            .expect("a reply");
        let response: Response = serde_json::from_slice(&reply).expect("decoding");
        assert_eq!(
            response.error().map(|e| e.kind),
            Some(anth_wire::ErrorKind::Invalid)
        );

        // The same connection must still work: a bad frame is not a hang-up.
        write_frame(&mut raw, br#"{"type":"host_info"}"#)
            .await
            .expect("writing");
        let reply = read_frame(&mut raw)
            .await
            .expect("reading")
            .expect("a reply");
        let response: Response = serde_json::from_slice(&reply).expect("decoding");
        assert!(matches!(response, Response::HostInfo(_)));
    }

    #[tokio::test]
    async fn connecting_to_a_missing_socket_says_so() {
        let path = SocketPath::new("absent");
        let error = Client::connect(&path.0).await.err().expect("must fail");
        assert_eq!(error.kind, anth_wire::ErrorKind::NotFound);
    }

    #[tokio::test]
    async fn binding_replaces_a_stale_socket() {
        let path = SocketPath::new("stale");
        let first = bind(&path.0).expect("first bind");
        drop(first);
        // The file is still on disk after the listener is dropped.
        assert!(path.0.exists(), "the stale socket file should remain");
        let _second = bind(&path.0).expect("a second bind must replace the stale file");
    }

    #[tokio::test]
    async fn binding_refuses_a_regular_file_instead_of_deleting_it() {
        let path = SocketPath::new("regular");
        std::fs::write(&path.0, b"important").expect("writing the file");

        let error = bind(&path.0).expect_err("must refuse");

        assert_eq!(error.kind, anth_wire::ErrorKind::Invalid);
        assert_eq!(
            std::fs::read(&path.0).expect("the file must survive"),
            b"important"
        );
    }
}

/// The shell: the lookup rules, and the routes built on them.
///
/// Every fixture is a real directory, because the rules worth testing here are
/// about the filesystem — a path that escapes, a symlink out of the tree, a file
/// that is not there — and a mock would simply agree with itself.
mod www {
    use crate::dispatch::Daemon;
    use crate::www::{content_type_for, denied, normalise};
    use axum::Router;
    use axum::body::Body;
    use axum::http::Request as HttpRequest;
    use std::path::Path;
    #[cfg(debug_assertions)]
    use std::path::PathBuf;
    use tower::ServiceExt;

    /// A fixture tree, removed on drop.
    ///
    /// Debug-only: the tests that use it exercise the filesystem lookup, which
    /// a release build does not compile. Without the gate they are dead code
    /// there and `-D warnings` turns that into a failed build.
    #[cfg(debug_assertions)]
    struct Shell(PathBuf);

    #[cfg(debug_assertions)]
    impl Shell {
        fn new(label: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let root = std::env::temp_dir()
                .join(format!("anth-www-{label}-{}-{nanos}", std::process::id()));
            std::fs::create_dir_all(&root).expect("creating the shell root");
            Self(root)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        /// Writes one file, creating its parents, and returns its path.
        fn write(&self, relative: &str, contents: &str) -> PathBuf {
            let path = self.0.join(relative);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("creating a fixture directory");
            }
            std::fs::write(&path, contents).expect("writing a fixture");
            path
        }

        /// The bytes `relative` would answer with, through the same lookup a
        /// request reaches.
        ///
        /// Debug-only, along with the tests below: a release build serves only
        /// what it embedded and compiles no filesystem reader at all, so there
        /// is no rule here to check.
        #[cfg(debug_assertions)]
        fn get(&self, relative: &str) -> Option<Vec<u8>> {
            crate::www::lookup(&self.0, relative).map(|asset| asset.bytes.into_owned())
        }
    }

    #[cfg(debug_assertions)]
    impl Drop for Shell {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_normal_path_is_kept() {
        assert_eq!(normalise("assets/app.js").as_deref(), Some("assets/app.js"));
    }

    #[test]
    fn redundant_separators_and_dots_collapse() {
        assert_eq!(normalise("./a//b.js").as_deref(), Some("a/b.js"));
        assert_eq!(normalise("a/b/./c.css").as_deref(), Some("a/b/c.css"));
    }

    /// The shell directory has no parent to clamp to, so a `..` is refused
    /// rather than resolved: resolving it would answer a file the shell never
    /// published.
    #[test]
    fn a_parent_component_is_refused() {
        for path in ["../secret", "a/../../secret", "/etc/passwd"] {
            assert_eq!(normalise(path), None, "{path} must be refused");
        }
    }

    /// The shell directory is tracked in git, so a clean checkout holds
    /// `.gitkeep`. Embedding it is harmless; serving it is not.
    #[cfg(debug_assertions)]
    #[test]
    fn a_dotfile_is_refused() {
        let dir = Shell::new("dotfile");
        dir.write(".gitkeep", "");
        dir.write(".env", "TOKEN=secret");

        assert_eq!(normalise(".gitkeep"), None);
        assert!(dir.get(".gitkeep").is_none());
        assert!(dir.get(".env").is_none());
    }

    #[cfg(debug_assertions)]
    #[test]
    fn an_asset_is_read_with_its_type() {
        let dir = Shell::new("asset");
        dir.write("assets/app.js", "console.log(1)");

        let bytes = dir.get("assets/app.js").expect("the asset exists");

        assert_eq!(bytes, b"console.log(1)");
    }

    #[cfg(debug_assertions)]
    #[test]
    fn a_missing_file_is_none() {
        let dir = Shell::new("missing");
        dir.write("index.html", "<h1>shell</h1>");

        assert!(dir.get("assets/nope.js").is_none());
    }

    /// The development page is on disk in this fixture and still unreachable,
    /// so a deployment that copied it cannot serve it by accident.
    #[cfg(debug_assertions)]
    #[test]
    fn the_harness_is_never_served() {
        let dir = Shell::new("harness");
        dir.write("harness.html", "<p>dev</p>");

        assert!(denied("harness.html"));
        assert!(
            dir.get("harness.html").is_none(),
            "the harness must not be reachable"
        );
    }

    /// The same guarantee, in a build whose files were embedded at compile
    /// time. `denied` is what both sources consult, and it needs no fixture,
    /// so the exclusion is checked in the profile that ships.
    #[test]
    fn the_harness_is_denied_by_name_in_every_profile() {
        assert!(denied("harness.html"));
        assert!(!denied("index.html"));
        assert!(!denied("assets/harness.html.js"));
        assert!(!denied(""));
    }

    /// A symlink inside the tree that points outside it must not be followed:
    /// the shell is a directory the daemon published, and this is what keeps
    /// that true when the bundle is not.
    #[cfg(all(unix, debug_assertions))]
    #[test]
    fn a_symlink_out_of_the_tree_is_refused() {
        let dir = Shell::new("symlink");
        // A second fixture root is the target: outside the served tree, which
        // is the case that matters, and owned by a `Drop` of its own, so the
        // test leaves nothing behind in the scratch directory.
        let target = Shell::new("symlink-target");
        let outside = target.write("secret.txt", "secret");
        std::os::unix::fs::symlink(&outside, dir.path().join("escape.txt"))
            .expect("creating the symlink");

        assert!(
            dir.get("escape.txt").is_none(),
            "a symlink leaving the tree must not be served"
        );
    }

    /// The table is short on purpose, and an unknown extension stays opaque
    /// rather than being guessed at.
    #[test]
    fn the_content_type_table_guesses_nothing() {
        assert_eq!(
            content_type_for(Path::new("a.css")),
            "text/css; charset=utf-8"
        );
        assert_eq!(content_type_for(Path::new("a.json")), "application/json");
        assert_eq!(content_type_for(Path::new("a.woff2")), "font/woff2");
        assert_eq!(
            content_type_for(Path::new("a.unknown")),
            "application/octet-stream"
        );
        assert_eq!(
            content_type_for(Path::new("no-extension")),
            "application/octet-stream"
        );
    }

    fn shell_router() -> Router {
        crate::tests::signed_router(
            Daemon::for_test(
                crate::plugins::registry::Registry::empty(),
                crate::tests::quiet(),
            ),
            "shell",
        )
    }

    async fn ask(uri: &str) -> (u16, String, String) {
        let response = shell_router()
            .oneshot(
                HttpRequest::builder()
                    .uri(uri)
                    .body(Body::empty())
                    .expect("a request"),
            )
            .await
            .expect("the router answers");

        let status = response.status().as_u16();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_string();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("a body");
        (
            status,
            content_type,
            String::from_utf8_lossy(&bytes).into_owned(),
        )
    }

    /// A typo in a client is a JSON `404`, never the index. Answering HTML here
    /// makes a broken request look like a working route until the JSON parse
    /// fails somewhere far away.
    #[tokio::test]
    async fn an_unknown_api_path_is_a_json_not_found() {
        let (status, content_type, body) = ask("/api/typo").await;

        assert_eq!(status, 404, "{body}");
        assert_eq!(content_type, "application/json");
        assert!(
            body.contains("no such API route"),
            "the body must say what is wrong: {body}"
        );
    }

    /// The catch-all must not shadow the routes that already existed.
    #[tokio::test]
    async fn the_api_routes_still_answer_before_the_catch_all() {
        let (status, _, body) = ask("/healthz").await;

        assert_eq!(status, 200);
        assert!(body.contains("ok"), "{body}");
    }

    /// With no shell built, the answer names the build step rather than
    /// looking like a missing page.
    #[tokio::test]
    async fn a_build_without_a_shell_says_so() {
        // Whether this machine has a shell is not the property under test: the
        // message is. Asking the real build made this test return early on a
        // machine with `www/` populated, which is every development machine.
        let missing = crate::api::shell_missing(false, "index.html");
        assert_eq!(missing.0.kind.http_status(), 404);
        assert!(
            missing.0.message.contains("bun run build"),
            "the message must name the build step: {}",
            missing.0.message
        );
    }

    #[test]
    fn a_built_shell_names_the_path_it_lacks() {
        let missing = crate::api::shell_missing(true, "assets/app.js");
        assert_eq!(missing.0.kind.http_status(), 404);
        assert!(
            missing.0.message.contains("assets/app.js"),
            "the message must name the path: {}",
            missing.0.message
        );
        assert!(
            !missing.0.message.contains("bun run build"),
            "a built shell must not be told to build: {}",
            missing.0.message
        );
    }

    /// A clean checkout has `www/` because `.gitkeep` is tracked, and no index
    /// because the shell is not. Treating the directory as the answer reported
    /// that state as a missing path.
    #[cfg(debug_assertions)]
    #[test]
    fn an_empty_tracked_shell_directory_is_not_a_shell() {
        let dir = Shell::new("only-gitkeep");
        dir.write(".gitkeep", "");

        assert!(!crate::www::available_at(dir.path()));
        dir.write("index.html", "<!doctype html>");
        assert!(crate::www::available_at(dir.path()));
    }

    /// The profile that ships must actually serve the shell it embedded.
    ///
    /// Every other test in this module uses a fixture tree, which a release
    /// build cannot do: it compiled the filesystem reader out. So this one asks
    /// the embedded copy, through the real router, for the one file the build
    /// script exists to require. Without it a release binary that embedded an
    /// empty `www/` would pass the whole suite and serve no UI in production.
    #[cfg(not(debug_assertions))]
    #[tokio::test]
    async fn a_release_build_serves_the_shell_it_embedded() {
        let (status, content_type, body) = ask("/").await;

        assert_eq!(status, 200, "the embedded shell must answer: {body}");
        assert_eq!(content_type, "text/html; charset=utf-8");
        assert!(
            body.contains("<anth-app"),
            "the index must mount the shell: {body}"
        );
    }
}

/// Serving a plugin's web assets: `/p/<id>/web/<module>`.
///
/// Driven through the real router and a real bundle on disk, because the part
/// worth testing is the decision about *which files may be read*, and a mocked
/// registry would let a broken allowlist pass.
mod plugin_web {
    use crate::dispatch::Daemon;
    use crate::plugins::registry::Registry;
    use axum::body::Body;
    use axum::http::Request as HttpRequest;
    use sha2::{Digest, Sha256};
    use std::path::{Path, PathBuf};
    use tower::ServiceExt;

    const GUEST: &str = r#"
(module
  (import "env" "anth_exec" (func $exec (param i32 i32 i32 i32) (result i64)))
  (import "env" "anth_log" (func $log (param i32 i32 i32)))
  (memory (export "memory") 1)
  (data (i32.const 1024) "\7b\22status\22:200,\22value\22:null\7d")
  (func (export "anth_abi_version") (result i32) (i32.const 3))
  (func (export "anth_alloc") (param i32) (result i32) (i32.const 2048))
  (func (export "anth_free") (param i32 i32))
  (func (export "anth_http") (param i32 i32) (result i64)
    (i64.or (i64.shl (i64.const 1024) (i64.const 32)) (i64.const 27)))
)
"#;

    struct Root(PathBuf);

    impl Root {
        fn new(label: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let path = std::env::temp_dir()
                .join(format!("anth-web-{label}-{}-{nanos}", std::process::id()));
            std::fs::create_dir_all(&path).expect("creating the root");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Root {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Writes a loadable bundle with one web module declared.
    fn bundle(root: &Path, id: &str, module: &str) -> PathBuf {
        let wasm = wat::parse_str(GUEST).expect("the guest compiles");
        let digest = hex(&Sha256::digest(&wasm));
        let dir = root.join(id);
        std::fs::create_dir_all(&dir).expect("creating the bundle");
        std::fs::write(dir.join("plugin.wasm"), &wasm).expect("writing the module");
        std::fs::write(
            dir.join("plugin.toml"),
            format!(
                r#"
[plugin]
id = "{id}"
version = "0.2.0"
name = "Web"
abi = 3

[wasm]
path = "plugin.wasm"
sha256 = "{digest}"

[[api]]
method = "GET"
path = "/greeting"

[[ui]]
slot = "dashboard.widget"
tag = "anth-web"
module = "{module}"
"#
            ),
        )
        .expect("writing the manifest");
        let file = dir.join(module);
        std::fs::create_dir_all(file.parent().expect("a parent")).expect("creating web");
        std::fs::write(&file, "export const widget = 1;").expect("writing the asset");
        dir
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    async fn get(root: &Path, uri: &str) -> (u16, String, String) {
        let router = crate::tests::signed_router(
            Daemon::for_test(Registry::load(root), crate::tests::quiet()),
            "web-module",
        );
        let response = router
            .oneshot(
                HttpRequest::builder()
                    .uri(uri)
                    .body(Body::empty())
                    .expect("a request"),
            )
            .await
            .expect("the router answers");
        let status = response.status().as_u16();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_string();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("a body");
        (
            status,
            content_type,
            String::from_utf8_lossy(&bytes).into_owned(),
        )
    }

    #[tokio::test]
    async fn a_declared_module_is_served_as_javascript() {
        let root = Root::new("served");
        bundle(root.path(), "org.test.web", "web/widget.js");

        let (status, content_type, body) = get(root.path(), "/p/org.test.web/web/widget.js").await;

        assert_eq!(status, 200, "body: {body}");
        assert_eq!(content_type, "text/javascript; charset=utf-8");
        assert_eq!(body, "export const widget = 1;");
    }

    /// The whole point of the allowlist: the wasm and the manifest sit in the
    /// same directory, and neither may be downloaded.
    #[tokio::test]
    async fn a_bundles_wasm_and_manifest_are_not_downloadable() {
        let root = Root::new("private");
        bundle(root.path(), "org.test.web", "web/widget.js");
        let router = crate::tests::signed_router(
            Daemon::for_test(Registry::load(root.path()), crate::tests::quiet()),
            "private-bundle",
        );

        for uri in [
            "/p/org.test.web/web/plugin.wasm",
            "/p/org.test.web/web/plugin.toml",
            "/p/org.test.web/web/../plugin.wasm",
            "/p/org.test.web/web/../plugin.toml",
            "/p/org.test.web/web/other.js",
        ] {
            let response = router
                .clone()
                .oneshot(
                    HttpRequest::builder()
                        .uri(uri)
                        .body(Body::empty())
                        .expect("a request"),
                )
                .await
                .expect("the router answers");
            assert_eq!(
                response.status().as_u16(),
                404,
                "`{uri}` must not be served"
            );
        }
    }

    /// Two different `404`s. A caller who typed the wrong plugin id must not be
    /// told that some other, real plugin fails to declare the module: the
    /// message is the only thing that says which half of `/p/<id>/<module>` was
    /// wrong, and it names an id the caller never used if it guesses.
    #[tokio::test]
    async fn the_two_not_found_reasons_do_not_get_confused() {
        let root = Root::new("reasons");
        bundle(root.path(), "org.test.web", "web/widget.js");

        let (status, _, unknown_plugin) = get(root.path(), "/p/org.test.nope/web/widget.js").await;
        assert_eq!(status, 404);
        assert!(
            !unknown_plugin.contains("declares no ui module"),
            "blamed a plugin that does not exist: {unknown_plugin}"
        );
        assert!(unknown_plugin.contains("org.test.nope"), "{unknown_plugin}");

        let (status, _, unknown_module) = get(root.path(), "/p/org.test.web/web/nope.js").await;
        assert_eq!(status, 404);
        assert!(
            unknown_module.contains("declares no ui module"),
            "{unknown_module}"
        );
    }

    #[tokio::test]
    async fn an_undeclared_plugin_is_a_not_found_that_names_it() {
        let root = Root::new("unknown");
        bundle(root.path(), "org.test.web", "web/widget.js");

        let (status, _, body) = get(root.path(), "/p/org.test.nope/web/widget.js").await;

        assert_eq!(status, 404);
        assert!(body.contains("org.test.nope"), "got: {body}");
    }

    /// A disabled bundle has no validated manifest, so it has nothing to
    /// declare. It must not read as "no such module": the reason is the answer.
    #[tokio::test]
    async fn a_disabled_plugin_explains_instead_of_serving() {
        let root = Root::new("disabled");
        let dir = bundle(root.path(), "org.test.web", "web/widget.js");
        std::fs::write(dir.join("plugin.wasm"), b"not the approved bytes")
            .expect("rewriting the module");

        let (status, _, body) = get(root.path(), "/p/org.test.web/web/widget.js").await;

        assert_eq!(status, 403);
        assert!(body.contains("disabled"), "got: {body}");
    }

    /// The two namespaces are disjoint: a bundle's module is served by `/p/`
    /// and its anchor by `/api/plugins/{id}/`, and each keeps working while the
    /// other route exists beside it.
    #[tokio::test]
    async fn a_module_and_an_anchor_are_served_by_their_own_namespaces() {
        let root = Root::new("anchor");
        bundle(root.path(), "org.test.web", "web/widget.js");

        let (status, _, body) = get(root.path(), "/p/org.test.web/web/widget.js").await;
        assert_eq!(status, 200, "the module: {body}");

        // `/p/greeting` is not a module, so it is refused as one rather than
        // being handed to the anchor route.
        let (status, _, body) = get(root.path(), "/p/greeting").await;
        assert_eq!(status, 404, "body: {body}");
        assert!(body.contains("no plugin"), "got: {body}");
    }
}

/// One line per request, which is the gap the log analysis found: four real
/// requests used to leave a single startup line behind.
mod request_log {
    use crate::dispatch::Daemon;
    use crate::log::{Capture, Format, Level, Logger};
    use crate::plugins::registry::Registry;
    use anth_wire::{Request, Response};
    use std::sync::Arc;

    fn daemon_at(level: Level) -> (Daemon, Capture) {
        let sink = Capture::default();
        let logger = Arc::new(Logger::new(level, Format::Json, Arc::new(sink.clone())));
        (Daemon::for_test(Registry::empty(), logger), sink)
    }

    #[tokio::test]
    async fn a_successful_request_leaves_one_line_with_status_and_duration() {
        let (daemon, sink) = daemon_at(Level::Info);

        let response = daemon.handle(Request::HostInfo).await;
        assert!(matches!(response, Response::HostInfo(_)));

        let lines = sink.lines();
        assert_eq!(lines.len(), 1, "one line per request, got {lines:?}");
        let parsed: serde_json::Value = serde_json::from_str(&lines[0]).expect("a JSON line");
        assert_eq!(parsed["target"], "request");
        assert_eq!(parsed["op"], "host-info");
        assert_eq!(parsed["status"], 200);
        assert_eq!(parsed["level"], "info");
        assert!(parsed["ms"].is_number(), "a duration is recorded: {parsed}");
    }

    /// A refused request is the one an operator goes looking for, so it has to be
    /// there, at `warn`, with the status the client actually receives.
    #[tokio::test]
    async fn a_refused_request_is_logged_at_warn_with_its_status() {
        let (daemon, sink) = daemon_at(Level::Info);

        let response = daemon
            .handle(Request::PluginCall {
                id: "org.absent".to_string(),
                pipe: anth_wire::PluginPipe::Direct,
                payload: "{}".to_string(),
            })
            .await;

        let status = match response {
            Response::Error(error) => error.kind.http_status(),
            other => panic!("expected a refusal, got {other:?}"),
        };
        let lines = sink.lines();
        assert_eq!(lines.len(), 1, "got {lines:?}");
        let parsed: serde_json::Value = serde_json::from_str(&lines[0]).expect("JSON");
        assert_eq!(parsed["level"], "warn", "a failure is not buried at info");
        assert_eq!(parsed["status"], status);
        assert_eq!(parsed["op"], "plugin-call");
    }

    /// The payload is the guest's text. Printing it would let a plugin write its
    /// own words into the host's request line.
    #[tokio::test]
    async fn the_plugin_payload_is_not_quoted_into_the_line() {
        let (daemon, sink) = daemon_at(Level::Info);

        daemon
            .handle(Request::PluginCall {
                id: "org.absent".to_string(),
                pipe: anth_wire::PluginPipe::Cli,
                payload: "anth-panel: forged line".to_string(),
            })
            .await;

        let lines = sink.lines();
        assert_eq!(lines.len(), 1, "got {lines:?}");
        assert!(
            !lines[0].contains("forged line"),
            "the payload must not reach the log: {lines:?}"
        );
    }

    #[tokio::test]
    async fn debug_adds_a_receipt_line_before_the_answer() {
        let (daemon, sink) = daemon_at(Level::Debug);

        daemon.handle(Request::PluginList).await;

        let lines = sink.lines();
        assert_eq!(lines.len(), 2, "got {lines:?}");
        let first: serde_json::Value = serde_json::from_str(&lines[0]).expect("JSON");
        let second: serde_json::Value = serde_json::from_str(&lines[1]).expect("JSON");
        assert_eq!(first["msg"], "received");
        assert_eq!(first["level"], "debug");
        assert_eq!(second["msg"], "answered");
    }

    #[tokio::test]
    async fn a_warn_threshold_keeps_the_refusal_and_drops_the_success() {
        let (daemon, sink) = daemon_at(Level::Warn);

        daemon.handle(Request::HostInfo).await;
        assert!(sink.lines().is_empty(), "info is below warn");

        daemon
            .handle(Request::PluginCall {
                id: "org.absent".to_string(),
                pipe: anth_wire::PluginPipe::Direct,
                payload: "{}".to_string(),
            })
            .await;
        assert_eq!(sink.lines().len(), 1, "a refusal survives");
    }
}

/// Anchor methods: a manifest may declare any of five, and every one of them
/// must be reachable at the same URL.
///
/// Driven through the real router over a real bundle, because the defect this
/// replaces was a route registered `get()` while the manifest accepted `POST`:
/// a unit test of the lookup would have passed throughout.
mod anchor_methods {
    use crate::dispatch::Daemon;
    use crate::plugins::registry::Registry;
    use axum::body::Body;
    use axum::http::Request as HttpRequest;
    use sha2::{Digest, Sha256};
    use std::path::{Path, PathBuf};
    use tower::ServiceExt;

    /// A guest that answers `ok`, so a method test asserts reachability rather
    /// than the plugin's own logic.
    const GUEST: &str = r#"
(module
  (import "env" "anth_exec" (func $exec (param i32 i32 i32 i32) (result i64)))
  (import "env" "anth_log" (func $log (param i32 i32 i32)))
  (memory (export "memory") 1)
  (data (i32.const 1024) "\7b\22status\22:200,\22value\22:null\7d")
  (func (export "anth_abi_version") (result i32) (i32.const 3))
  (func (export "anth_alloc") (param i32) (result i32) (i32.const 2048))
  (func (export "anth_free") (param i32 i32))
  (func (export "anth_http") (param i32 i32) (result i64)
    (i64.or (i64.shl (i64.const 1024) (i64.const 32)) (i64.const 27)))
)
"#;

    struct Root(PathBuf);

    impl Root {
        fn new(label: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let path = std::env::temp_dir().join(format!(
                "anth-method-{label}-{}-{nanos}",
                std::process::id()
            ));
            std::fs::create_dir_all(&path).expect("creating the root");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Root {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// One bundle declaring one anchor with `method`.
    fn bundle(root: &Path, id: &str, method: &str, anchor: &str) {
        let wasm = wat::parse_str(GUEST).expect("the guest compiles");
        let digest: String = Sha256::digest(&wasm)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let dir = root.join(id);
        std::fs::create_dir_all(&dir).expect("creating the bundle");
        std::fs::write(dir.join("plugin.wasm"), &wasm).expect("writing the module");
        std::fs::write(
            dir.join("plugin.toml"),
            format!(
                r#"
[plugin]
id = "{id}"
version = "0.2.0"
name = "Method"
abi = 3

[wasm]
path = "plugin.wasm"
sha256 = "{digest}"

[[api]]
method = "{method}"
path = "{anchor}"
"#
            ),
        )
        .expect("writing the manifest");
    }

    async fn send(root: &Path, method: &str, uri: &str, body: &[u8]) -> (u16, String) {
        let router = crate::tests::signed_router(
            Daemon::for_test(Registry::load(root), crate::tests::quiet()),
            "anchor",
        );
        let response = router
            .oneshot(
                HttpRequest::builder()
                    .method(method)
                    .uri(uri)
                    .body(Body::from(body.to_vec()))
                    .expect("a request"),
            )
            .await
            .expect("the router answers");
        let status = response.status().as_u16();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("a body");
        (status, String::from_utf8_lossy(&bytes).into_owned())
    }

    /// Every method the manifest accepts must reach the anchor it declared.
    /// Before this, a `POST`, `PUT`, `PATCH` or `DELETE` anchor answered `405`
    /// while `/api/plugins` listed it as available.
    #[tokio::test]
    async fn every_method_the_manifest_accepts_reaches_its_anchor() {
        for method in ["GET", "POST", "PUT", "PATCH", "DELETE"] {
            let root = Root::new(method);
            bundle(root.path(), "org.test.m", method, "/op");

            let (status, body) = send(root.path(), method, "/api/plugins/org.test.m/op", b"").await;
            assert_eq!(status, 200, "{method} must reach its anchor, got {body}");
        }
    }

    /// The method a caller used is the method the plugin is told about: a
    /// `POST` must not arrive at the guest as a `GET`.
    #[tokio::test]
    async fn a_method_other_than_get_is_not_rewritten_before_the_plugin() {
        let root = Root::new("rewritten");
        bundle(root.path(), "org.test.m", "POST", "/op");

        let response = crate::tests::signed_router(
            Daemon::for_test(Registry::load(root.path()), crate::tests::quiet()),
            "rewritten-method",
        )
        .oneshot(
            HttpRequest::builder()
                .method("GET")
                .uri("/api/plugins/org.test.m/op")
                .body(Body::empty())
                .expect("a request"),
        )
        .await
        .expect("the router answers");

        assert_eq!(
            response.status().as_u16(),
            404,
            "GET must not resolve an anchor declared POST"
        );
    }

    /// A body reaches the plugin as the bytes the caller sent, and no body is
    /// `null` rather than an empty string, so a plugin can tell them apart.
    #[test]
    fn the_envelope_carries_the_callers_method_body_and_query() {
        let with_body = crate::api::http_payload(
            "POST",
            "/submit".to_string(),
            Some("a=1"),
            br#"{"name":"Ana"}"#,
        )
        .unwrap_or_else(|error| panic!("a valid envelope: {}", error.0.message));

        assert_eq!(with_body["method"], serde_json::json!("POST"));
        assert_eq!(with_body["path"], serde_json::json!("/submit"));
        assert_eq!(with_body["query"]["a"], serde_json::json!("1"));
        assert_eq!(with_body["body"], serde_json::json!(r#"{"name":"Ana"}"#));

        let without = crate::api::http_payload("GET", "/submit".to_string(), None, b"")
            .unwrap_or_else(|error| panic!("a valid envelope: {}", error.0.message));
        assert_eq!(without["body"], serde_json::Value::Null);
    }

    /// A body that is not text is refused, not lossily converted: the plugin's
    /// `HttpRequest::body` is a `String`, so handing it replacement characters
    /// would be the panel rewriting the caller's request.
    #[test]
    fn a_body_that_is_not_utf8_is_a_400() {
        let error = match crate::api::http_payload("POST", "/x".to_string(), None, &[0xff, 0xfe]) {
            Ok(_) => panic!("invalid UTF-8 must be refused"),
            Err(error) => error,
        };

        assert_eq!(error.0.kind, anth_wire::ErrorKind::Invalid);
    }
}

/// `host-info --watch`: the interval, the frames, and the loop over a real daemon.
///
/// The loop is the client's, so there is no daemon-side watch to test. What is
/// worth pinning is that the two output shapes differ for the reason they exist,
/// that a repeated watch really asks more than once, and that a reader going
/// away ends it as a success rather than a failure.
mod watch {
    use crate::tests::spawn_daemon;
    use crate::watch::{Frames, watch_into};
    use anth_wire::parse_interval;
    use std::io::{self, Write};
    use std::process::ExitCode;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    #[test]
    fn an_interval_needs_a_unit_and_is_converted() {
        assert_eq!(parse_interval("500ms").unwrap(), Duration::from_millis(500));
        assert_eq!(parse_interval("2s").unwrap(), Duration::from_secs(2));
        assert_eq!(parse_interval("1.5s").unwrap(), Duration::from_millis(1500));
        assert_eq!(parse_interval("1m").unwrap(), Duration::from_secs(60));
        assert_eq!(parse_interval("1h").unwrap(), Duration::from_secs(3600));
        // Whitespace is a fact of shell input, not a different interval.
        assert_eq!(parse_interval(" 2s ").unwrap(), Duration::from_secs(2));
    }

    /// A bare number is refused rather than guessed: `2` read as seconds is
    /// right, `2` read as milliseconds is a thousand snapshots a second.
    #[test]
    fn an_interval_without_a_unit_is_refused() {
        let problem = parse_interval("2").unwrap_err();
        assert!(problem.contains("unit suffix"), "{problem}");
    }

    #[test]
    fn an_interval_must_be_positive_and_in_range() {
        for raw in ["0s", "-1s", "abc s", "0.0001ms", "25h"] {
            assert!(
                parse_interval(raw).is_err(),
                "`{raw}` must be refused, not silently accepted"
            );
        }
    }

    /// On a terminal each frame replaces the last; through a pipe frames are
    /// separated by a blank line. Both are asserted as exact bytes, because the
    /// difference is the whole reason `redraw` exists.
    #[test]
    fn a_redraw_overwrites_and_a_pipe_separates() {
        let mut redrawn = Vec::new();
        let mut frames = Frames::new(&mut redrawn, true);
        frames.write_frame("a\nb").unwrap();
        frames.write_frame("c").unwrap();
        let text = String::from_utf8(redrawn).unwrap();
        // Two lines in the first frame, so the cursor walks up two.
        assert_eq!(text, "a\nb\n\x1b[2A\x1b[Jc\n");

        let mut piped = Vec::new();
        let mut frames = Frames::new(&mut piped, false);
        frames.write_frame("a").unwrap();
        frames.write_frame("b").unwrap();
        let text = String::from_utf8(piped).unwrap();
        assert_eq!(text, "a\n\nb\n");
        assert!(
            !text.contains('\x1b'),
            "a redirected log must carry no escapes: {text:?}"
        );
    }

    /// A writer that accepts `allowed` frames and then refuses, the way `| head`
    /// closes the pipe. The bytes and the count are shared, because `Frames`
    /// takes the writer by value and the test still has to read what it wrote.
    #[derive(Clone, Default)]
    struct Head {
        seen: Arc<Mutex<String>>,
        frames: Arc<Mutex<usize>>,
        allowed: usize,
    }

    impl Write for Head {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let mut frames = self.frames.lock().unwrap();
            if *frames >= self.allowed {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "head went away"));
            }
            self.seen
                .lock()
                .unwrap()
                .push_str(std::str::from_utf8(buf).unwrap());
            // `write_frame` issues one `write` per line, so the frame boundary is
            // the newline that ends it — the same thing a reader sees.
            *frames += buf.iter().filter(|b| **b == b'\n').count();
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// The whole path: a real listener, a real `Client`, the real renderer, and
    /// a writer that stops once it has been handed more than two frames.
    ///
    /// Two frames is what proves the loop repeats; asserting only that it
    /// returned would pass for a watch that drew once and stopped.
    #[tokio::test]
    async fn a_watch_draws_more_than_one_frame_and_stops_when_the_reader_leaves() {
        let path = spawn_daemon("watch-frames").await;
        // Three table frames are about 54 newlines, so a budget of 40 lets two
        // frames through whole and refuses during the third. The point is that
        // more than one frame arrived, and the budget is what ends the run
        // instead of the test timing out.
        // Three table frames are about 54 newlines, so a budget of 40 lets two
        // frames through whole and refuses during the third: the budget ends the
        // run instead of the test waiting on a timeout.
        let head = Head {
            allowed: 40,
            ..Head::default()
        };
        let seen = Arc::clone(&head.seen);

        let code = watch_into(&path.0, Duration::from_millis(20), Frames::new(head, false)).await;

        // `BrokenPipe` is the reader's decision, not a failure.
        assert_eq!(code, ExitCode::SUCCESS);
        let text = seen.lock().unwrap().clone();
        // The top border is drawn once per frame, so counting it counts
        // snapshots without depending on a label's spelling.
        assert!(
            text.matches('┌').count() >= 2,
            "the loop must draw more than one snapshot, got: {text}"
        );
    }

    /// A watch that cannot reach the daemon fails like any other client command,
    /// rather than looping on a connection that was never made.
    #[tokio::test]
    async fn a_watch_without_a_daemon_reports_the_failure() {
        let missing = crate::tests::SocketPath::new("watch-absent");

        let code = watch_into(
            &missing.0,
            Duration::from_millis(20),
            Frames::new(io::sink(), false),
        )
        .await;

        // `Client::connect` reports a missing socket as `NotFound`, and the exit
        // code must come from that kind rather than a hardcoded 1.
        assert_eq!(
            code,
            ExitCode::from(anth_wire::ErrorKind::NotFound.exit_code())
        );
    }
}

/// Where the daemon looks for its socket, its bundles and its settings.
///
/// Every case passes its own environment and its own file, because the thing
/// under test is the *order* the four sources are consulted in. A test that read
/// the real environment would pass on a machine that happens to have
/// `ANTH_PLUGINS` set and fail on one that does not.
mod config {
    use crate::config::{DEFAULT_PLUGINS, DEFAULT_SOCKET, Paths, path, resolve};
    use std::path::{Path, PathBuf};

    /// A settings file that sorts a label, with the given contents.
    struct File(PathBuf);

    impl File {
        fn new(label: &str, text: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let path = std::env::temp_dir().join(format!(
                "anth-config-{label}-{}-{nanos}.toml",
                std::process::id()
            ));
            std::fs::write(&path, text).expect("writing the settings file");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for File {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn no_env(_: &str) -> Option<String> {
        None
    }

    fn with_env(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| value.to_string())
        }
    }

    /// With nothing set anywhere, both paths are relative to the working
    /// directory, which is what makes a checkout runnable with no flags.
    #[test]
    fn an_empty_supply_yields_the_relative_defaults() {
        let paths = resolve(None, None, None, &no_env, None).expect("no settings to fail on");

        assert_eq!(
            paths,
            Paths {
                socket: PathBuf::from(DEFAULT_SOCKET),
                plugins: PathBuf::from(DEFAULT_PLUGINS),
                readable: PathBuf::from(crate::config::DEFAULT_READABLE),
                terminal: false,
                auth: true,
            }
        );
        assert!(!paths.socket.is_absolute());
        assert!(!paths.plugins.is_absolute());
    }

    /// The default is the protection, so an untouched checkout requires a
    /// session.
    #[test]
    fn an_empty_supply_leaves_authentication_on() {
        let paths = resolve(None, None, None, &no_env, None).expect("no settings to fail on");

        // The default is the protection. A test that flipped this to `false`
        // would be pinning the wrong side of the switch.
        assert!(paths.auth);
    }

    /// Only the file can turn auth off, and only when it says so.
    #[test]
    fn the_auth_table_is_opt_out_and_the_rest_of_the_file_is_unchanged() {
        let file = File::new("auth-off", "[auth]\nenabled = false\n");
        let paths = resolve(None, None, None, &no_env, Some(file.path())).expect("a valid file");

        assert!(!paths.auth);
        // Turning auth off must not disturb the socket or the bundle root: the
        // two settings are read by the same pass and independent of each other.
        assert_eq!(paths.socket, PathBuf::from(DEFAULT_SOCKET));
        assert_eq!(paths.plugins, PathBuf::from(DEFAULT_PLUGINS));
    }

    /// A `[auth]` table that says nothing leaves the default in place.
    #[test]
    fn an_empty_auth_table_keeps_authentication_on() {
        let file = File::new("auth-empty", "[auth]\n");
        let paths = resolve(None, None, None, &no_env, Some(file.path())).expect("a valid file");

        assert!(paths.auth);
    }

    /// An explicit `true` reads the same as no key at all, so a file written by
    /// a tool that spells out every setting is not the odd one out.
    #[test]
    fn an_explicit_true_matches_the_default() {
        let file = File::new("auth-on", "[auth]\nenabled = true\n");
        let paths = resolve(None, None, None, &no_env, Some(file.path())).expect("a valid file");

        assert!(paths.auth);
    }

    /// The flag wins over everything, because it is the one thing the operator
    /// typed at that moment.
    #[test]
    fn a_flag_wins_over_the_environment_and_the_file() {
        let file = File::new(
            "flag",
            "socket = \"/from-file.sock\"\nplugins = \"/from-file/plugins\"\n",
        );
        let env = with_env(&[
            ("ANTH_SOCKET", "/from-env.sock"),
            ("ANTH_PLUGINS", "/from-env/plugins"),
        ]);

        let paths = resolve(
            Some("/from-flag.sock"),
            Some("/from-flag/plugins"),
            None,
            &env,
            Some(file.path()),
        )
        .expect("a valid file");

        assert_eq!(paths.socket, PathBuf::from("/from-flag.sock"));
        assert_eq!(paths.plugins, PathBuf::from("/from-flag/plugins"));
    }

    /// The environment wins over the file, so a service unit can override one
    /// setting without rewriting a file the package owns.
    #[test]
    fn the_environment_wins_over_the_file() {
        let file = File::new("env", "socket = \"/from-file.sock\"\n");
        let env = with_env(&[("ANTH_SOCKET", "/from-env.sock")]);

        let paths = resolve(None, None, None, &env, Some(file.path())).expect("a valid file");

        assert_eq!(paths.socket, PathBuf::from("/from-env.sock"));
        // The file sets only `socket`, so `plugins` has nothing to read and
        // stays relative to the working directory.
        assert_eq!(paths.plugins, PathBuf::from(DEFAULT_PLUGINS));
    }

    /// A key the file sets and nothing else does is the file's, and a key it
    /// omits stays relative.
    #[test]
    fn the_file_fills_what_the_environment_left_alone() {
        let file = File::new("file", "plugins = \"/srv/anth/plugins\"\n");
        let paths = resolve(None, None, None, &no_env, Some(file.path())).expect("a valid file");

        assert_eq!(paths.plugins, PathBuf::from("/srv/anth/plugins"));
        assert_eq!(paths.socket, PathBuf::from(DEFAULT_SOCKET));
    }

    /// A file written on purpose that will not parse is refused. Answering it
    /// with defaults would run the daemon with settings nobody chose.
    #[test]
    fn a_file_that_will_not_parse_is_refused() {
        let file = File::new("broken", "socket = [unterminated\n");

        let error = resolve(None, None, None, &no_env, Some(file.path())).expect_err("must refuse");

        assert_eq!(error.kind, anth_wire::ErrorKind::Invalid);
        assert!(
            error.message.contains("will not parse"),
            "{}",
            error.message
        );
    }

    /// A file that is not there is the default settings, not an error: that is
    /// a fresh checkout.
    #[test]
    fn a_missing_file_is_the_defaults() {
        let paths = resolve(
            None,
            None,
            None,
            &no_env,
            Some(Path::new("/nonexistent/anth/config.toml")),
        )
        .expect("a missing file is not a failure");

        assert_eq!(paths.socket, PathBuf::from(DEFAULT_SOCKET));
        assert_eq!(paths.plugins, PathBuf::from(DEFAULT_PLUGINS));
    }

    /// An empty environment variable is "not set", matching how the log
    /// settings treat one, so a service that exports an empty string behaves
    /// like one that exports nothing.
    #[test]
    fn an_empty_environment_value_is_ignored() {
        let env = with_env(&[("ANTH_SOCKET", ""), ("ANTH_PLUGINS", "   ")]);

        let paths = resolve(None, None, None, &env, None).expect("no settings to fail on");

        assert_eq!(paths.socket, PathBuf::from(DEFAULT_SOCKET));
        assert_eq!(paths.plugins, PathBuf::from(DEFAULT_PLUGINS));
    }

    /// The settings file itself is chosen the same way, relative by default.
    #[test]
    fn the_config_file_path_is_relative_by_default() {
        assert_eq!(path(None, &no_env), PathBuf::from("config.toml"));
        assert_eq!(
            path(None, &with_env(&[("ANTH_CONFIG", "/etc/anth/config.toml")])),
            PathBuf::from("/etc/anth/config.toml")
        );
        assert_eq!(
            path(Some("/flag/config.toml"), &no_env),
            PathBuf::from("/flag/config.toml")
        );
    }
}

/// Terminal requests at the dispatch layer.
///
/// The registry's own tests cover the rules; these cover the one thing only
/// `dispatch` can answer: whether a daemon that was never granted terminals
/// refuses them, and whether the grant actually reaches a real shell.
mod terminal_dispatch {
    use crate::dispatch::Daemon;
    use crate::plugins::registry::Registry;
    use anth_wire::{ErrorKind, Request, Response};

    fn daemon(terminal: bool) -> Daemon {
        Daemon::for_test(Registry::empty(), crate::tests::quiet())
            .with_terminal(crate::terminal::App { enabled: terminal })
    }

    #[tokio::test]
    async fn a_daemon_without_the_grant_refuses_every_terminal_request() {
        let daemon = daemon(false);

        let requests = [
            Request::TerminalOpen {
                program: "sh".to_string(),
                cols: 80,
                rows: 24,
            },
            Request::TerminalRead {
                id: "t1".to_string(),
                wait_ms: 0,
            },
            Request::TerminalWrite {
                id: "t1".to_string(),
                bytes: b"x".to_vec(),
            },
            Request::TerminalResize {
                id: "t1".to_string(),
                cols: 80,
                rows: 24,
            },
            Request::TerminalClose {
                id: "t1".to_string(),
            },
        ];

        for request in requests {
            let named = format!("{request:?}");
            match daemon.handle(request).await {
                Response::Error(error) => {
                    assert_eq!(
                        error.kind,
                        ErrorKind::Forbidden,
                        "{named} must be refused, not answered differently"
                    );
                    assert!(
                        error.message.contains("[terminal] enabled"),
                        "the refusal must name the setting: {}",
                        error.message
                    );
                }
                other => panic!("{named} must be refused, got {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn an_open_read_write_close_round_trip_reaches_a_real_shell() {
        let daemon = daemon(true);

        let opened = daemon
            .handle(Request::TerminalOpen {
                program: "sh".to_string(),
                cols: 100,
                rows: 40,
            })
            .await;
        let (id, cols, rows) = match opened {
            Response::TerminalOpened {
                id,
                program,
                cols,
                rows,
            } => {
                assert_eq!(program, "sh", "the answer must echo what was started");
                (id, cols, rows)
            }
            other => panic!("expected an opened terminal, got {other:?}"),
        };
        assert_eq!((cols, rows), (100, 40));

        daemon
            .handle(Request::TerminalWrite {
                id: id.clone(),
                bytes: b"echo dispatched\n".to_vec(),
            })
            .await;

        // The shell is asynchronous, so the answer is polled rather than
        // assumed: a single read would be racing it.
        let mut seen = String::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline && !seen.contains("dispatched") {
            match daemon
                .handle(Request::TerminalRead {
                    id: id.clone(),
                    wait_ms: 200,
                })
                .await
            {
                Response::TerminalOutput { bytes, .. } => {
                    seen.push_str(&String::from_utf8_lossy(&bytes));
                }
                other => panic!("expected output, got {other:?}"),
            }
        }
        assert!(seen.contains("dispatched"), "saw {seen:?}");

        // A resize is answered, and the session is still usable afterwards.
        let resized = daemon
            .handle(Request::TerminalResize {
                id: id.clone(),
                cols: 120,
                rows: 50,
            })
            .await;
        assert!(matches!(resized, Response::Accepted));

        let closed = daemon
            .handle(Request::TerminalClose { id: id.clone() })
            .await;
        assert!(matches!(closed, Response::Accepted));

        // After close the handle must be gone rather than answering for a
        // descriptor that no longer exists.
        match daemon
            .handle(Request::TerminalRead { id, wait_ms: 0 })
            .await
        {
            Response::Error(error) => assert_eq!(error.kind, ErrorKind::NotFound),
            other => panic!("a closed handle must be unknown, got {other:?}"),
        }
        assert_eq!(daemon.terminals().len(), 0);
    }

    #[tokio::test]
    async fn a_program_outside_the_allowlist_is_refused() {
        let daemon = daemon(true);
        let response = daemon
            .handle(Request::TerminalOpen {
                program: "cat".to_string(),
                cols: 80,
                rows: 24,
            })
            .await;

        match response {
            Response::Error(error) => {
                assert_eq!(error.kind, ErrorKind::Forbidden);
                assert!(error.message.contains("cat"), "{}", error.message);
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
    }
}

/// `host-info --watch` as clap parses it.
///
/// The flag is optional and its value optional too, which is three shapes clap
/// could get wrong silently: a bare `--watch` taking the *next* word as its
/// interval, or an absent value reading as `Some("")`, would both parse.
mod watch_cli {
    use crate::{Cli, Command};
    use clap::Parser;

    fn watch_of(args: &[&str]) -> Option<String> {
        let mut argv = vec!["anth-panel", "host-info"];
        argv.extend_from_slice(args);
        match Cli::try_parse_from(argv).expect("parsing").command {
            Some(Command::HostInfo { watch }) => watch,
            other => panic!("expected the host-info subcommand, got {other:?}"),
        }
    }

    #[test]
    fn no_flag_means_one_snapshot() {
        assert_eq!(watch_of(&[]), None);
    }

    /// A bare `--watch` must use the documented default rather than swallowing
    /// whatever follows it.
    #[test]
    fn a_bare_watch_takes_the_default_interval() {
        assert_eq!(watch_of(&["--watch"]), Some("2s".to_string()));
    }

    #[test]
    fn an_explicit_interval_is_kept_as_written() {
        assert_eq!(watch_of(&["--watch", "500ms"]), Some("500ms".to_string()));
        assert_eq!(
            watch_of(&["--watch", "1m"]),
            Some("1m".to_string()),
            "a following word is the interval, not a leftover positional"
        );
    }

    /// An interval the parser cannot read must reach the command rather than
    /// fail in clap: the message that names `ms, s, m or h` comes from the
    /// interval parser, and it is the one an operator can act on.
    #[test]
    fn an_unreadable_interval_still_parses_and_is_refused_later() {
        assert_eq!(watch_of(&["--watch", "nope"]), Some("nope".to_string()));
        assert!(anth_wire::parse_interval("nope").is_err());
    }
}

/// The SSE watch route, driven through the real router.
///
/// The body is read as bytes and asserted as the SSE wire format, because the
/// format *is* the contract here: an `EventSource` in the browser parses those
/// exact lines, so a change that looked harmless (renaming the event, dropping
/// the `data:` prefix) would break the widget with every Rust test still green.
mod sse {
    use crate::tests::api::test_router;
    use axum::body::Body;
    use axum::http::Request as HttpRequest;
    use futures_util::StreamExt;
    use std::time::{Duration, Instant};
    use tower::ServiceExt;

    /// Reads whatever the stream produced before `within`, then stops.
    ///
    /// The body is consumed chunk by chunk rather than with `to_bytes`, which
    /// only returns at EOF or at its byte limit: a watch has neither, so a
    /// single `to_bytes` would wait for a deadline and throw away every frame it
    /// had already received. Accumulating per chunk is what makes the read
    /// assert on bytes that actually arrived.
    async fn read_for(uri: &str, within: Duration) -> String {
        let response = test_router()
            .oneshot(
                HttpRequest::builder()
                    .uri(uri)
                    .body(Body::empty())
                    .expect("a valid request"),
            )
            .await
            .expect("the router must answer");

        assert_eq!(response.status().as_u16(), 200, "{uri} must open a stream");
        assert!(
            response
                .headers()
                .get("content-type")
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.starts_with("text/event-stream")),
            "an SSE route must say so in its content type"
        );

        let mut stream = response.into_body().into_data_stream();
        let mut text = String::new();
        let deadline = tokio::time::Instant::now() + within;

        loop {
            let next = tokio::time::timeout_at(deadline, stream.next()).await;
            match next {
                Ok(Some(Ok(chunk))) => text.push_str(&String::from_utf8_lossy(&chunk)),
                Ok(Some(Err(error))) => panic!("reading the stream failed: {error}"),
                Ok(None) | Err(_) => break,
            }
        }
        text
    }

    /// Frames are `event: <name>` lines followed by one `data:` each.
    fn frames(text: &str, event: &str) -> Vec<String> {
        text.split("\n\n")
            .filter(|block| block.contains(&format!("event: {event}")))
            .map(|block| {
                block
                    .lines()
                    .filter_map(|line| line.strip_prefix("data: "))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .collect()
    }

    #[tokio::test]
    async fn a_watch_opens_with_a_snapshot_and_names_its_event() {
        let text = read_for(
            "/api/host-info/watch?interval=20ms",
            Duration::from_millis(300),
        )
        .await;

        let snapshots = frames(&text, "host-info");
        assert!(
            !snapshots.is_empty(),
            "the first frame must arrive without waiting a whole interval: {text:?}"
        );

        // The payload is the same JSON shape the one-shot route answers, because
        // it is the same `HostInfo` — the widget parses one shape either way.
        let first: serde_json::Value =
            serde_json::from_str(&snapshots[0]).expect("a frame carries JSON");
        assert!(
            first["hostname"].as_str().is_some_and(|s| !s.is_empty()),
            "{first}"
        );
        assert!(first["os"]["name"].is_string(), "{first}");

        // The retry hint belongs to the first frame; a browser uses it to know
        // how long to wait before reconnecting.
        assert!(text.contains("retry:"), "no retry hint in {text:?}");
    }

    #[tokio::test]
    async fn a_watch_keeps_answering_at_the_interval_it_was_given() {
        let started = Instant::now();
        let text = read_for(
            "/api/host-info/watch?interval=20ms",
            Duration::from_millis(250),
        )
        .await;
        let elapsed = started.elapsed();

        let count = frames(&text, "host-info").len();
        // A 20ms interval over 250ms cannot yield fewer than a few frames. The
        // bound is loose on purpose: a busy CI machine slows the ticks, and the
        // assertion that matters is "more than one", not an exact count.
        assert!(
            count >= 3,
            "{count} frames in {elapsed:?} is not a repeating watch: {text:?}"
        );
    }

    /// An interval the CLI would refuse is refused here too, before any byte of
    /// the stream is written, so the client gets a status rather than a stream
    /// that opens and then says nothing.
    #[tokio::test]
    async fn a_bad_interval_is_a_400_before_the_stream_opens() {
        for uri in [
            "/api/host-info/watch?interval=2",
            "/api/host-info/watch?interval=0s",
            "/api/host-info/watch?interval=nope",
        ] {
            let response = test_router()
                .oneshot(
                    HttpRequest::builder()
                        .uri(uri)
                        .body(Body::empty())
                        .expect("a valid request"),
                )
                .await
                .expect("the router must answer");

            assert_eq!(response.status().as_u16(), 400, "{uri} must be refused");
        }
    }

    /// The route and the CLI flag accept exactly the same strings, because they
    /// call the same parser. This is the assertion that keeps them from drifting
    /// when one of the two grows a spelling the other lacks.
    #[test]
    fn the_route_and_the_flag_share_one_interval_grammar() {
        for raw in ["500ms", "2s", "1.5s", "1m", "1h", " 2s "] {
            assert!(
                anth_wire::parse_interval(raw).is_ok(),
                "`{raw}` must be accepted by both transports"
            );
        }
        for raw in ["2", "0s", "-1s", "2d"] {
            assert!(
                anth_wire::parse_interval(raw).is_err(),
                "`{raw}` must be refused by both transports"
            );
        }
    }
}

/// The stream the watch is built from, with the answer supplied by the test.
///
/// Driving `snapshot_stream` directly is what makes the failure path testable:
/// a real daemon answers `HostInfo` and cannot be asked to fail on cue, and the
/// behaviour worth pinning is that one error ends the stream rather than being
/// retried forever.
mod sse_stream {
    use crate::api::snapshot_stream;
    use anth_wire::{Response, WireError};
    use futures_util::StreamExt;

    /// Collects at most `limit` frames, so a stream that never ends fails the
    /// test by count instead of hanging it.
    async fn take(
        mut stream: impl futures_util::Stream<
            Item = Result<axum::response::sse::Event, std::convert::Infallible>,
        > + Unpin,
        limit: usize,
    ) -> Vec<String> {
        let mut seen = Vec::new();
        while seen.len() < limit {
            match stream.next().await {
                Some(Ok(event)) => seen.push(format!("{event:?}")),
                Some(Err(never)) => match never {},
                None => break,
            }
        }
        seen
    }

    #[tokio::test]
    async fn a_refusal_ends_the_stream_after_one_error_event() {
        let mut answers = vec![
            Response::Error(WireError::internal("the host went away")),
            // If the stream kept going it would answer this second one, so its
            // absence is what proves the first error stopped it.
            Response::HostInfo(anth_wire::HostInfo {
                hostname: "must never be sent".into(),
                ..crate::tests::blank_host_info()
            }),
        ]
        .into_iter();

        let ticks = tokio::time::interval(std::time::Duration::from_millis(10));
        let stream = snapshot_stream(ticks, || {
            let next = answers
                .next()
                .unwrap_or(Response::Error(WireError::internal(
                    "asked more than twice",
                )));
            async move { next }
        });

        let events = take(Box::pin(stream), 5).await;
        assert_eq!(
            events.len(),
            1,
            "an error must end the stream, not feed the next frame: {events:?}"
        );
        assert!(events[0].contains("error"), "{events:?}");
    }

    #[tokio::test]
    async fn a_snapshot_carries_the_host_info_event_name() {
        let ticks = tokio::time::interval(std::time::Duration::from_millis(10));
        let stream = snapshot_stream(ticks, || async {
            Response::HostInfo(crate::tests::blank_host_info())
        });

        let events = take(Box::pin(stream), 2).await;
        assert_eq!(events.len(), 2, "a watch must not stop after one frame");
        for event in &events {
            assert!(event.contains("host-info"), "{event}");
        }
    }
}

/// The two lifecycle actions: what each replaces, and what each one refuses.
///
/// Every test here drives the seam rather than the host. A test that called
/// `System::reboot` would not report a failure — it would take down the machine
/// the suite runs on — and a test that called `System::restart` would replace
/// the test binary with itself and never report either. `Recorded` is what makes
/// the assertions able to fail.
mod lifecycle {
    use std::ffi::OsString;

    use crate::lifecycle::{Actions, Lifecycle, MAX_REBOOT_DELAY_S, Recorded};

    fn recording(allow_reboot: bool) -> (Lifecycle, std::sync::Arc<Recorded>) {
        Lifecycle::recorded(allow_reboot)
    }

    #[test]
    fn a_restart_replaces_this_process_with_its_own_command_line() {
        let (lifecycle, recorded) = recording(false);

        lifecycle.restart();

        let calls = recorded.restarts.lock().unwrap();
        assert_eq!(calls.len(), 1, "one restart, one exec");
        let argv = &calls[0];
        assert!(!argv.is_empty(), "exec needs a program");
        // The program a restart asks for is the one running: a restart that
        // exec'd something else would be a different operation wearing the name.
        let running = std::env::current_exe().expect("the test binary has a path");
        let asked = std::path::Path::new(&argv[0]);
        assert!(
            asked.file_name() == running.file_name()
                || asked.to_string_lossy() == running.to_string_lossy(),
            "restart asked for {}, which is not {}",
            asked.display(),
            running.display()
        );
    }

    #[test]
    fn a_restart_keeps_the_arguments_it_was_started_with() {
        let (lifecycle, recorded) = recording(false);

        lifecycle.restart();

        let calls = recorded.restarts.lock().unwrap();
        let argv = &calls[0];
        // The daemon is replaced by the command that started it, not by a fresh
        // invocation: dropping `--config` or `--socket` would restart with
        // different settings than the ones on disk, which is the opposite of what
        // a restart is for.
        let expected: Vec<OsString> = std::env::args_os().collect();
        assert_eq!(*argv, expected, "the whole command line must survive");
    }

    #[test]
    fn a_daemon_without_the_switch_refuses_a_reboot_without_running_one() {
        let (lifecycle, recorded) = recording(false);

        let refused = lifecycle.authorize_reboot(0).expect_err("must refuse");

        assert_eq!(refused.kind, anth_wire::ErrorKind::Forbidden);
        assert!(
            recorded.reboots.lock().unwrap().is_empty(),
            "a refused reboot must not reach the host"
        );
    }

    #[test]
    fn the_refusal_names_the_flag_that_would_allow_it() {
        let (lifecycle, _recorded) = recording(false);

        let refused = lifecycle.authorize_reboot(0).expect_err("must refuse");

        assert!(
            refused.message.contains("--allow-reboot"),
            "an operator reading this must learn how to allow it: {}",
            refused.message
        );
    }

    #[test]
    fn a_reboot_past_the_limit_is_refused_before_it_reaches_the_host() {
        let (lifecycle, recorded) = recording(true);

        let refused = lifecycle
            .authorize_reboot(MAX_REBOOT_DELAY_S + 1)
            .expect_err("must refuse");

        assert_eq!(refused.kind, anth_wire::ErrorKind::Invalid);
        assert!(
            recorded.reboots.lock().unwrap().is_empty(),
            "a delay outside the limit must not schedule anything"
        );
    }

    #[test]
    fn the_longest_allowed_delay_is_accepted() {
        let (lifecycle, recorded) = recording(true);

        lifecycle
            .authorize_reboot(MAX_REBOOT_DELAY_S)
            .expect("the limit is inclusive");

        // Authorising is not scheduling: the caller answers first, then performs.
        assert!(recorded.reboots.lock().unwrap().is_empty());
    }

    #[test]
    fn an_allowed_reboot_is_performed_with_the_delay_it_was_given() {
        let (lifecycle, recorded) = recording(true);

        lifecycle.authorize_reboot(30).expect("allowed");
        lifecycle.perform_reboot(30).expect("recorded");

        assert_eq!(*recorded.reboots.lock().unwrap(), vec![30]);
    }

    #[test]
    fn a_restart_needs_no_switch() {
        // The asymmetry is the point: replacing this process depends on nothing
        // the daemon does not already have, so it is not gated like the reboot.
        let (lifecycle, recorded) = recording(false);

        lifecycle.restart();

        assert_eq!(recorded.restarts.lock().unwrap().len(), 1);
    }

    /// Points `PATH` at a directory holding one stand-in command, runs `change`,
    /// and restores the environment.
    fn with_path_of(
        directory: &std::path::Path,
        change: impl FnOnce() -> Result<(), anth_wire::WireError>,
    ) -> Result<(), anth_wire::WireError> {
        let saved = std::env::var_os("PATH");
        // SAFETY: no test in this binary spawns a process concurrently, and the
        // window is one call. `set_var` is unsafe in edition 2024 because a
        // concurrent read would be undefined behaviour.
        unsafe { std::env::set_var("PATH", directory) };
        let outcome = change();
        match saved {
            Some(saved) => unsafe { std::env::set_var("PATH", saved) },
            None => unsafe { std::env::remove_var("PATH") },
        }
        outcome
    }

    fn stand_in(name: &str, body: &str, scratch: &str) -> std::path::PathBuf {
        // `scratch` returns a settings *file* inside its directory, so the
        // script has to sit beside it rather than at the path it hands back.
        let settings = crate::tests::scratch(scratch);
        let directory = settings
            .parent()
            .expect("a scratch directory")
            .to_path_buf();
        let script = directory.join(name);
        std::fs::write(&script, format!("#!/bin/sh\n{body}\n")).expect("a script");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
            .expect("an executable");
        directory
    }

    #[test]
    fn a_failing_reboot_command_is_reported_rather_than_assumed_to_work() {
        // `System` tries `systemctl` first, then `reboot`. A `PATH` holding only
        // a failing `reboot` makes the first attempt `NotFound` and the second
        // exit non-zero, which must be reported rather than swallowed: an answer
        // that says "rebooting" when the host is not is the worst outcome here.
        let directory = stand_in("reboot", "exit 1", "lifecycle-reboot-fail");

        let error = with_path_of(&directory, || crate::lifecycle::System.reboot(0))
            .expect_err("a host that failed to reboot must not report success");

        assert_eq!(error.kind, anth_wire::ErrorKind::Internal);
        assert!(
            error.message.contains("reboot"),
            "the message must name what failed: {}",
            error.message
        );
    }

    #[test]
    fn a_reboot_command_that_succeeds_is_not_an_error() {
        let directory = stand_in("reboot", "exit 0", "lifecycle-reboot-ok");

        with_path_of(&directory, || crate::lifecycle::System.reboot(0))
            .expect("a reboot command that exited 0 is a success");
    }

    #[test]
    fn the_real_actions_and_the_recorder_are_the_same_trait() {
        // `Recorded` is `cfg(test)`; this keeps the substitution type-checked, so
        // a change to the trait cannot leave the recorder behind.
        let _real: &dyn Actions = &crate::lifecycle::System;
        let recorded = <std::sync::Arc<Recorded> as Default>::default();
        let _fake: &dyn Actions = recorded.as_ref();
    }
}

/// The two routes that change what is running: `/api/restart` and `/api/reboot`.
///
/// `test_router` builds a daemon whose lifecycle is a recorder, so a request
/// here records instead of replacing this test binary or rebooting the machine.
/// The refusal test builds its own daemon with the switch off, because that is
/// the state a daemon started without `--allow-reboot` is in and the state a
/// `403` has to come from.
mod lifecycle_routes {
    use axum::body::Body;
    use axum::http::Request as HttpRequest;
    use serde_json::json;
    use tower::ServiceExt;

    use crate::tests::{quiet, signed_router};

    async fn post(router: axum::Router, uri: &str, body: &str) -> (u16, serde_json::Value) {
        let response = router
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri(uri)
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .expect("a valid request"),
            )
            .await
            .expect("the router must answer");
        let status = response.status().as_u16();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .expect("a readable body");
        let body = serde_json::from_slice(&bytes).expect("the body must be JSON");
        (status, body)
    }

    #[tokio::test]
    async fn a_restart_is_accepted_rather_than_reported_complete() {
        let (status, body) = post(crate::tests::api::test_router(), "/api/restart", "").await;

        // `202`: the replacement happens after this answer is written. A `200`
        // would claim the new process is already serving, which it cannot be.
        assert_eq!(status, 202);
        assert_eq!(body["restarting"], json!(true));
        // The pid is the pid the new process will have, because `execv` keeps
        // it. A caller reconnecting and finding a different one is talking to a
        // restart that went through `spawn`, which orphans the old process.
        assert!(body["pid"].as_u64().is_some(), "{}", body["pid"]);
    }

    #[tokio::test]
    async fn a_restart_says_what_it_costs() {
        let (_status, body) = post(crate::tests::api::test_router(), "/api/restart", "").await;

        // A caller that does not know a restart applies the settings and drops
        // the terminals cannot tell it from a reload that does neither.
        assert_eq!(body["applies_settings"], json!(true));
        assert_eq!(body["drop_terminals"], json!(true));
    }

    #[tokio::test]
    async fn a_reboot_without_the_switch_is_forbidden_and_does_nothing() {
        // The state a daemon started without `--allow-reboot` is in. Built
        // explicitly because `for_test` allows everything: its recorder exists
        // so a request cannot reach the host, not to model the switch.
        let (lifecycle, recorded) = crate::lifecycle::Lifecycle::recorded(false);
        let daemon =
            crate::dispatch::Daemon::for_test(crate::plugins::registry::Registry::empty(), quiet())
                .with_lifecycle(lifecycle);
        let (status, body) =
            post(signed_router(daemon, "reboot-refused"), "/api/reboot", "{}").await;

        assert_eq!(status, 403);
        assert!(
            body["error"]
                .as_str()
                .unwrap_or_default()
                .contains("--allow-reboot"),
            "{}",
            body["error"]
        );
        assert!(
            recorded.reboots.lock().unwrap().is_empty(),
            "a refused reboot must not reach the host"
        );
    }

    #[tokio::test]
    async fn a_reboot_with_the_switch_is_accepted() {
        let (lifecycle, _recorded) = crate::lifecycle::Lifecycle::recorded(true);
        let daemon =
            crate::dispatch::Daemon::for_test(crate::plugins::registry::Registry::empty(), quiet())
                .with_lifecycle(lifecycle);
        let (status, body) = post(
            signed_router(daemon, "reboot-allowed"),
            "/api/reboot",
            r#"{"delay_s":10}"#,
        )
        .await;

        assert_eq!(status, 202);
        assert_eq!(body["rebooting"], json!(true));
        assert_eq!(body["delay_s"], json!(10));
    }

    #[tokio::test]
    async fn a_reboot_delay_past_the_limit_is_a_bad_request() {
        let (lifecycle, _recorded) = crate::lifecycle::Lifecycle::recorded(true);
        let daemon =
            crate::dispatch::Daemon::for_test(crate::plugins::registry::Registry::empty(), quiet())
                .with_lifecycle(lifecycle);
        let (status, body) = post(
            signed_router(daemon, "reboot-too-long"),
            "/api/reboot",
            r#"{"delay_s":301}"#,
        )
        .await;

        assert_eq!(status, 400);
        assert!(
            body["error"].as_str().unwrap_or_default().contains("300"),
            "{}",
            body["error"]
        );
    }

    #[tokio::test]
    async fn neither_route_answers_a_get() {
        // A link or a prefetch must not restart a daemon. Only `POST` is routed,
        // so a `GET` is a `405` rather than a restart.
        for uri in ["/api/restart", "/api/reboot"] {
            let response = crate::tests::api::test_router()
                .oneshot(
                    HttpRequest::builder()
                        .uri(uri)
                        .body(Body::empty())
                        .expect("a valid request"),
                )
                .await
                .expect("the router must answer");
            assert_eq!(response.status().as_u16(), 405, "{uri}");
        }
    }

    #[tokio::test]
    async fn a_restart_without_a_session_is_unauthorized() {
        // Built directly rather than through `test_router`, which attaches a
        // token: this is the state an unauthenticated caller is in, and a
        // restart that answered it would let a page restart a daemon.
        let daemon =
            crate::dispatch::Daemon::for_test(crate::plugins::registry::Registry::empty(), quiet());
        let auth = std::sync::Arc::new(crate::auth::guard::Auth::load(
            crate::tests::scratch("restart-anonymous"),
            true,
        ));
        let router = crate::api::router(std::sync::Arc::new(daemon), auth);

        let response = router
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/api/restart")
                    .body(Body::empty())
                    .expect("a valid request"),
            )
            .await
            .expect("the router must answer");

        assert_eq!(response.status().as_u16(), 401);
    }
}
