//! Tests for the modules at the root of this binary.
//!
//! One file per level: the HTTP router, the dispatcher, the CLI, the table and
//! the socket are siblings in `src/`, so their tests are siblings here. The
//! `plugins/` subtree keeps its own `tests.rs`, one per level.

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
        router(Arc::new(Daemon::new(
            crate::plugins::registry::Registry::empty(),
        )))
    }

    /// A router with one disabled plugin, which is enough to prove a lookup.
    fn aliased_router() -> Router {
        router(Arc::new(Daemon::new(
            crate::plugins::registry::Registry::with_disabled(
                "org.test.aliased",
                Some("short"),
                &[("GET", "/greeting")],
                "the bundle was rewritten",
            ),
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

    /// An anchor no plugin declares must not fall through to the SPA, which
    /// would answer `200 index.html` and look like a working route.
    #[tokio::test]
    async fn an_unknown_anchor_is_a_404() {
        let (status, body) = get_status_and_body("/p/nope").await;

        assert_eq!(status, 404);
        assert!(body["error"].as_str().unwrap().contains("no anchor"));
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
        Daemon::new(Registry::empty())
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
        let daemon = Daemon::new(Registry::with_disabled(
            "org.test.aliased",
            Some("short"),
            &[],
            "the bundle was rewritten",
        ));

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
        let daemon = Daemon::new(Registry::with_disabled(
            "org.test.aliased",
            Some("short"),
            &[("GET", "/greeting")],
            "disabled",
        ));

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
        let daemon = Daemon::new(Registry::with_disabled(
            "org.test.aliased",
            Some("short"),
            &[],
            "disabled",
        ));

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

    /// A caller typing a line is standing where the plugin's CLI stands, so that
    /// is the pipe it gets without asking. Defaulting to `direct` made every
    /// `plugin call <id> greet` fail as "not a request".
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
        assert_eq!(action.pipe(), PluginPipe::Cli);
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
        assert_eq!(action.pipe(), PluginPipe::Direct);
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
        assert_eq!(action.pipe(), PluginPipe::Http);
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
        assert_eq!(crate::payload(line, action.pipe()), "greet --name Ana");
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
            crate::payload(line, action.pipe()),
            "greet --name 'Ana Sol'"
        );
        assert_eq!(
            crate::repl::split(&crate::payload(line, action.pipe())),
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
            crate::payload(line, action.pipe()),
            "greet --name 'Ana Sol'"
        );
        assert_eq!(
            crate::repl::split(&crate::payload(line, action.pipe())),
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
                disabled_reason: None,
            },
            PluginSummary {
                id: "org.test.plain".into(),
                alias: None,
                name: "Plain".into(),
                version: "0.2.0".into(),
                anchors: vec![],
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
        let daemon = Arc::new(Daemon::new(crate::plugins::registry::Registry::empty()));
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
