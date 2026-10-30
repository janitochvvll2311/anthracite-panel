//! Tests for every module in this crate.
//!
//! One file per level: the modules above hold the contract, and this asserts
//! the shapes the host depends on. `tests/pipes.rs` stays separate because it
//! links the crate as a dependency, which is the only way to check the exports
//! a wasm guest actually sees.

use super::*;

#[test]
fn the_abi_version_follows_the_feature() {
    #[cfg(feature = "abi4")]
    assert_eq!(ABI_VERSION, 4);
    #[cfg(not(feature = "abi4"))]
    assert_eq!(ABI_VERSION, 3);
}

/// The plugin's own vocabulary: actions, requests and statuses.
mod message {
    use super::*;
    use alloc::string::String;
    use serde_json::Value;

    use serde_json::json;

    const GREETING: Action = Action::new("greeting");
    const ADD_PEER: Action = Action::new("add-peer");
    const NESTED: Action = Action::new("wg.show_dump");

    #[test]
    fn an_action_keeps_its_name() {
        assert_eq!(GREETING.as_str(), "greeting");
        assert_eq!(ADD_PEER.as_str(), "add-peer");
        assert_eq!(NESTED.as_str(), "wg.show_dump");
    }

    #[test]
    fn every_status_maps_to_an_http_code() {
        assert_eq!(Status::Ok.code(), 200);
        assert_eq!(Status::Created.code(), 201);
        assert_eq!(Status::NoContent.code(), 204);
        assert_eq!(Status::BadRequest.code(), 400);
        assert_eq!(Status::Forbidden.code(), 403);
        assert_eq!(Status::NotFound.code(), 404);
        assert_eq!(Status::Conflict.code(), 409);
        assert_eq!(Status::Failed.code(), 500);
    }

    /// The CLI convention this repeats lives in `anth-wire`: a malformed request
    /// is `2` so a script can tell "you asked wrong" from "it broke".
    #[test]
    fn exit_codes_follow_the_wire_convention() {
        assert_eq!(Status::Ok.exit_code(), 0);
        assert_eq!(Status::Created.exit_code(), 0);
        assert_eq!(Status::NoContent.exit_code(), 0);
        assert_eq!(Status::BadRequest.exit_code(), 2);
        assert_eq!(Status::NotFound.exit_code(), 1);
        assert_eq!(Status::Forbidden.exit_code(), 1);
        assert_eq!(Status::Conflict.exit_code(), 1);
        assert_eq!(Status::Failed.exit_code(), 1);
    }

    /// The keys are compared, not the text: a JSON object has no order, and
    /// `serde_json` writes them sorted. Pinning the string would fail on a
    /// harmless reordering and hide a real change.
    fn envelope(status: &Response) -> Value {
        serde_json::from_str(&status.to_envelope()).expect("the envelope is JSON")
    }

    #[test]
    fn a_response_is_the_envelope_the_host_reads() {
        assert_eq!(
            envelope(&Response::ok(json!("anchored"))),
            json!({ "status": 200, "body": "anchored" })
        );
    }

    /// A `null` value must drop `body` entirely: the host serves `204` only when
    /// the key is absent, and serves a present `null` as the text `null`.
    #[test]
    fn a_null_value_omits_the_body() {
        assert_eq!(
            envelope(&Response::new(Status::NoContent, Value::Null)),
            json!({ "status": 204 })
        );
    }

    #[test]
    fn an_error_carries_its_detail() {
        assert_eq!(
            envelope(&Response::failed("no binary")),
            json!({ "status": 500, "body": { "error": "no binary" } })
        );
    }

    #[test]
    fn an_unknown_command_lists_the_known_ones() {
        let response =
            Response::unknown_command("frobnicate", &[("status", GREETING), ("peers", ADD_PEER)]);

        assert_eq!(response.status, Status::BadRequest);
        assert_eq!(
            response.value["error"],
            json!("no such subcommand `frobnicate`; try one of: status, peers")
        );
    }

    #[test]
    fn a_mismatched_value_is_a_400_naming_the_action() {
        let request = Request::new(ADD_PEER, json!({ "interface": 7 }));

        let response = request.parse_value::<Peer>().unwrap_err();

        assert_eq!(response.status, Status::BadRequest);
        assert!(
            response.value["error"]
                .as_str()
                .unwrap()
                .contains("add-peer"),
            "the message must name the action: {}",
            response.value
        );
    }

    #[derive(serde::Deserialize, Debug)]
    struct Peer {
        #[allow(dead_code)]
        interface: String,
    }
}

/// One type per pipe, and the parsing each one owns.
mod transport {
    use super::*;
    use alloc::string::{String, ToString};
    use alloc::vec::Vec;

    const STATUS: Action = Action::new("status");

    fn command_line(line: &str) -> CliRequest {
        CliRequest::new(line)
    }

    fn words(line: &str) -> Vec<String> {
        command_line(line).words().unwrap()
    }

    #[test]
    fn the_line_is_kept_exactly_as_it_arrived() {
        let request = command_line("greet --name \"Ana Sol\"");

        assert_eq!(request.line(), "greet --name \"Ana Sol\"");
    }

    /// The payload the host sends is a JSON string, so the quotes around it are
    /// the encoding rather than part of the line.
    #[test]
    fn a_json_string_payload_decodes_to_the_line_inside_it() {
        let request = CliRequest::parse(r#""peers --interface wg0""#).unwrap();

        assert_eq!(request.line(), "peers --interface wg0");
    }

    /// An object payload is what the previous contract sent, so it must be a
    /// loud `400` rather than a command named `{"argv":...}`.
    #[test]
    fn an_old_argv_payload_is_a_400_naming_the_expected_shape() {
        let refused = CliRequest::parse(r#"{"argv":["greet"]}"#).unwrap_err();

        assert_eq!(refused.status, Status::BadRequest);
        let message = refused.value["error"].as_str().unwrap();
        assert!(message.contains("JSON string"), "got: {message}");
    }

    #[test]
    fn a_subcommand_and_positionals_are_separated() {
        let request = command_line("peers wg0 wg1");

        assert_eq!(request.command().unwrap(), "peers");
        assert_eq!(request.positional().unwrap(), ["wg0", "wg1"]);
        assert!(!request.flag("verbose"));
        assert_eq!(request.option("interface"), None);
    }

    /// The reason the line is kept as text instead of arriving pre-split: a
    /// host that joined an `argv` loses the boundary, so the quotes are the only
    /// thing that keeps `"Ana Sol"` one word.
    #[test]
    fn a_quoted_argument_with_a_space_stays_one_word() {
        assert_eq!(
            words("greet --name \"Ana Sol\""),
            ["greet", "--name", "Ana Sol"]
        );
        assert_eq!(
            words("greet --name 'Ana Sol'"),
            ["greet", "--name", "Ana Sol"]
        );
    }

    #[test]
    fn an_escaped_space_stays_one_word() {
        assert_eq!(words("greet Ana\\ Sol"), ["greet", "Ana Sol"]);
    }

    #[test]
    fn single_quotes_are_literal() {
        assert_eq!(words("greet 'a \\n b'"), ["greet", "a \\n b"]);
    }

    #[test]
    fn double_quotes_resolve_only_the_two_escapes() {
        assert_eq!(words(r#"greet "a\"b""#), ["greet", "a\"b"]);
        assert_eq!(words(r#"greet "a\nb""#), ["greet", "a\\nb"]);
    }

    #[test]
    fn an_unterminated_quote_is_a_400() {
        let refused = command_line("greet \"Ana").words().unwrap_err();

        assert_eq!(refused.status, Status::BadRequest);
        assert!(refused.value["error"].as_str().unwrap().contains("quote"));
    }

    #[test]
    fn long_flags_are_booleans() {
        let request = command_line("status --verbose");

        assert!(request.flag("verbose"));
        assert!(request.positional().unwrap().is_empty());
    }

    /// A flag followed by another flag stays a boolean: consuming the next word
    /// as a value would swallow `--json` and leave the caller mystified.
    #[test]
    fn a_flag_before_another_flag_takes_no_value() {
        let request = command_line("status --verbose --json");

        assert!(request.flag("verbose"));
        assert!(request.flag("json"));
        assert_eq!(request.option("verbose"), None);
    }

    #[test]
    fn a_long_option_takes_the_next_word() {
        assert_eq!(
            command_line("peers --interface wg0").option("interface"),
            Some("wg0".to_string())
        );
    }

    #[test]
    fn a_long_option_takes_an_equals_form() {
        assert_eq!(
            command_line("peers --interface=wg0").option("interface"),
            Some("wg0".to_string())
        );
    }

    #[test]
    fn a_long_flag_takes_an_equals_form_as_a_value() {
        assert_eq!(
            command_line("status --verbose=true").option("verbose"),
            Some("true".to_string())
        );
    }

    #[test]
    fn a_short_option_takes_the_next_word() {
        assert_eq!(
            command_line("peers -i wg0").option("i"),
            Some("wg0".to_string())
        );
    }

    /// `-abc` is one flag, not `-a -b -c`: a plugin may name a subcommand with
    /// several letters and the parser must not invent a different meaning.
    #[test]
    fn a_bundled_short_word_stays_one_flag() {
        let request = command_line("status -abc");

        assert!(request.flag("abc"));
        assert!(!request.flag("a"));
    }

    #[test]
    fn a_single_dash_is_a_positional() {
        assert_eq!(command_line("upload -").positional().unwrap(), ["-"]);
    }

    /// Everything after `--` is a word, even when it looks like a flag: that is
    /// how a caller passes a value that starts with a dash.
    #[test]
    fn a_double_dash_ends_the_flags() {
        let request = command_line("peers -- --not-a-flag");

        assert_eq!(request.positional().unwrap(), ["--not-a-flag"]);
        assert!(!request.flag("not-a-flag"));
    }

    #[test]
    fn an_empty_line_is_a_bad_request() {
        assert_eq!(
            command_line("").command().unwrap_err().status,
            Status::BadRequest
        );
    }

    #[test]
    fn a_leading_flag_is_a_bad_request_naming_it() {
        let refused = command_line("--verbose").command().unwrap_err();

        assert_eq!(refused.status, Status::BadRequest);
        assert!(
            refused.value["error"]
                .as_str()
                .unwrap()
                .contains("--verbose")
        );
    }

    #[test]
    fn the_value_carries_every_class_of_word() {
        let request = command_line("peers wg0 --verbose --interface=wg1 -- -x");

        assert_eq!(
            request.value().unwrap(),
            serde_json::json!({
                "command": "peers",
                "args": ["wg0", "-x"],
                "flags": { "verbose": true },
                "options": { "interface": "wg1" },
            })
        );
    }

    #[test]
    fn an_alias_resolves_to_its_action() {
        let request = command_line("status");

        assert_eq!(request.action(&[("status", STATUS)]).unwrap(), STATUS);
    }

    #[test]
    fn an_unknown_alias_names_the_known_ones() {
        let refused = command_line("frobnicate")
            .action(&[("status", STATUS)])
            .unwrap_err();

        assert_eq!(
            refused.value["error"],
            serde_json::json!("no such subcommand `frobnicate`; try one of: status")
        );
    }

    #[test]
    fn the_http_path_gets_one_leading_slash() {
        let request = HttpRequest {
            method: "GET".to_string(),
            path: "status".to_string(),
            query: Default::default(),
            body: None,
        };
        assert_eq!(request.path(), "/status");

        let request = HttpRequest {
            method: "GET".to_string(),
            path: "//status".to_string(),
            query: Default::default(),
            body: None,
        };
        assert_eq!(
            request.path(),
            "/status",
            "a doubled slash is the same anchor"
        );
    }

    #[test]
    fn an_unknown_method_is_a_400_naming_it() {
        let request = HttpRequest {
            method: "TRACE".to_string(),
            path: "/status".to_string(),
            query: Default::default(),
            body: None,
        };

        assert!(
            request.method().unwrap_err().value["error"]
                .as_str()
                .unwrap()
                .contains("TRACE")
        );
    }

    #[test]
    fn a_body_is_parsed_only_when_it_is_there() {
        let request = HttpRequest {
            method: "POST".to_string(),
            path: "/peers".to_string(),
            query: Default::default(),
            body: Some(r#"{"interface":"wg0"}"#.to_string()),
        };

        #[derive(serde::Deserialize, PartialEq, Debug)]
        struct Body {
            interface: String,
        }

        assert_eq!(
            request.body_json::<Body>().unwrap(),
            Some(Body {
                interface: "wg0".to_string()
            })
        );
    }

    #[test]
    fn a_bad_body_is_a_400() {
        let request = HttpRequest {
            method: "POST".to_string(),
            path: "/peers".to_string(),
            query: Default::default(),
            body: Some("not json".to_string()),
        };

        assert_eq!(
            request.body_json::<serde_json::Value>().unwrap_err().status,
            Status::BadRequest
        );
    }

    #[test]
    fn a_query_value_is_read_by_name() {
        let request = HttpRequest::parse(
            r#"{"method":"GET","path":"/status","query":{"verbose":"1"},"body":null}"#,
        )
        .unwrap();

        assert_eq!(request.query("verbose"), Some("1"));
        assert_eq!(request.query("missing"), None);
    }
}
