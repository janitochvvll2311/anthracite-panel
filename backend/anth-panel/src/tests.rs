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

/// The HTTP transport, which must answer exactly what the socket does.
mod api {
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

    fn test_router() -> Router {
        router(Arc::new(Daemon::for_test(
            crate::plugins::registry::Registry::empty(),
            crate::tests::quiet(),
        )))
    }

    /// A router with one disabled plugin, which is enough to prove a lookup.
    fn aliased_router() -> Router {
        router(Arc::new(Daemon::for_test(
            crate::plugins::registry::Registry::with_disabled(
                "org.test.aliased",
                Some("short"),
                &[("GET", "/greeting")],
                "the bundle was rewritten",
            ),
            crate::tests::quiet(),
        )))
    }

    async fn get_status_and_body(uri: &str) -> (u16, serde_json::Value) {
        ask(test_router(), uri).await
    }

    async fn ask(router: Router, uri: &str) -> (u16, serde_json::Value) {
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
        assert!(matches!(cli.command, Some(Command::HostInfo)));
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
        let Startup::RunFirst(command) = startup(Some(Command::HostInfo)) else {
            panic!("a client command must run before the prompt");
        };
        assert!(matches!(*command, Command::HostInfo));
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
mod socket {
    use crate::dispatch::Daemon;
    use crate::socket::{Client, bind, serve};
    use anth_wire::{Request, Response, read_frame, write_frame};
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio::net::UnixStream;

    /// A socket path unique to this test, removed on drop.
    struct SocketPath(PathBuf);

    impl SocketPath {
        fn new(label: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            Self(
                std::env::temp_dir()
                    .join(format!("anth-{label}-{}-{nanos}.sock", std::process::id())),
            )
        }
    }

    impl Drop for SocketPath {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    /// Starts a daemon on a unique path and returns the path once it is bound.
    async fn spawn_daemon(label: &str) -> SocketPath {
        let path = SocketPath::new(label);
        let listener = bind(&path.0).expect("binding the test socket");
        let daemon = Arc::new(Daemon::for_test(
            crate::plugins::registry::Registry::empty(),
            crate::tests::quiet(),
        ));
        tokio::spawn(async move {
            let _ = serve(listener, daemon).await;
        });
        path
    }

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
    use crate::api::router;
    use crate::dispatch::Daemon;
    use crate::www::{content_type_for, denied, normalise};
    use axum::Router;
    use axum::body::Body;
    use axum::http::Request as HttpRequest;
    use std::path::Path;
    #[cfg(debug_assertions)]
    use std::path::PathBuf;
    use std::sync::Arc;
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
        router(Arc::new(Daemon::for_test(
            crate::plugins::registry::Registry::empty(),
            crate::tests::quiet(),
        )))
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
    use crate::api::router;
    use crate::dispatch::Daemon;
    use crate::plugins::registry::Registry;
    use axum::body::Body;
    use axum::http::Request as HttpRequest;
    use sha2::{Digest, Sha256};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
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
        let router = router(Arc::new(Daemon::for_test(
            Registry::load(root),
            crate::tests::quiet(),
        )));
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
        let router = router(Arc::new(Daemon::for_test(
            Registry::load(root.path()),
            crate::tests::quiet(),
        )));

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
    use crate::api::router;
    use crate::dispatch::Daemon;
    use crate::plugins::registry::Registry;
    use axum::body::Body;
    use axum::http::Request as HttpRequest;
    use sha2::{Digest, Sha256};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
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
        let router = router(Arc::new(Daemon::for_test(
            Registry::load(root),
            crate::tests::quiet(),
        )));
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

        let response = router(Arc::new(Daemon::for_test(
            Registry::load(root.path()),
            crate::tests::quiet(),
        )))
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
            }
        );
        assert!(!paths.socket.is_absolute());
        assert!(!paths.plugins.is_absolute());
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
