use anth_panel::cli::{Cli, Command, WatchArgs};
use anth_panel::{handle_response, request_for};
use anth_proto::{Request, Response};
use clap::Parser;

#[test]
fn request_for_echo_builds_an_echo_request() {
    let command = Command::Echo {
        message: "hello".to_string(),
    };

    match request_for(&command) {
        Some(Request::Echo { message }) => assert_eq!(message, "hello"),
        other => panic!("expected an Echo request, got {other:?}"),
    }
}

#[test]
fn request_for_host_info_builds_a_host_info_request() {
    for command in [
        Command::HostInfo(WatchArgs::default()),
        Command::HostInfo(WatchArgs {
            watch: Some("5s".to_string()),
        }),
    ] {
        match request_for(&command) {
            Some(Request::HostInfo) => {}
            other => panic!("expected a HostInfo request, got {other:?}"),
        }
    }
}

#[test]
fn request_for_is_none_for_locally_served_commands() {
    assert!(request_for(&Command::Daemon).is_none());
    assert!(request_for(&Command::Api(Default::default())).is_none());
    assert!(request_for(&Command::Run(Default::default())).is_none());
    assert!(request_for(&Command::Terminal(Default::default())).is_none());
}

#[test]
fn cli_parses_a_host_info_subcommand() {
    let cli = Cli::try_parse_from(["anth-panel", "host-info"]).unwrap();
    assert_eq!(cli.config_path, None);
    match cli.command {
        Command::HostInfo(args) => assert_eq!(args.watch, None),
        other => panic!("expected HostInfo, got {other:?}"),
    }
}

#[test]
fn cli_accepts_a_config_flag_before_host_info() {
    let cli =
        Cli::try_parse_from(["anth-panel", "--config", "/tmp/anth.toml", "host-info"]).unwrap();
    assert_eq!(cli.config_path.as_deref(), Some("/tmp/anth.toml"));
    assert!(matches!(cli.command, Command::HostInfo(_)));
}

#[test]
fn cli_parses_an_echo_subcommand() {
    let cli = Cli::try_parse_from(["anth-panel", "echo", "hello"]).unwrap();
    assert_eq!(cli.config_path, None);

    match cli.command {
        Command::Echo { message } => assert_eq!(message, "hello"),
        other => panic!("expected Echo, got {other:?}"),
    }
}

#[test]
fn cli_accepts_a_config_flag_before_the_subcommand() {
    let cli =
        Cli::try_parse_from(["anth-panel", "--config", "/tmp/anth.toml", "echo", "hi"]).unwrap();
    assert_eq!(cli.config_path.as_deref(), Some("/tmp/anth.toml"));

    let cli = Cli::try_parse_from(["anth-panel", "-c", "/tmp/short.toml", "echo", "hi"]).unwrap();
    assert_eq!(cli.config_path.as_deref(), Some("/tmp/short.toml"));
}

#[test]
fn cli_requires_a_subcommand() {
    assert!(Cli::try_parse_from(["anth-panel"]).is_err());
}

#[test]
fn cli_rejects_an_echo_without_a_message() {
    assert!(Cli::try_parse_from(["anth-panel", "echo"]).is_err());
}

#[test]
fn handle_response_accepts_ok_and_error_without_panicking() {
    handle_response(&Response::Ok {
        message: "done".to_string(),
    });
    handle_response(&Response::Error {
        detail: "boom".to_string(),
    });
}

#[test]
fn cli_watch_flag_defaults_to_two_seconds_when_bare() {
    let cli = Cli::try_parse_from(["anth-panel", "host-info", "--watch"]).unwrap();

    match cli.command {
        Command::HostInfo(args) => assert_eq!(args.watch.as_deref(), Some("2s")),
        other => panic!("expected HostInfo, got {other:?}"),
    }
}

#[test]
fn cli_watch_flag_accepts_an_explicit_interval() {
    for interval in ["500ms", "5s", "1m", "1.5s", "2h"] {
        let cli = Cli::try_parse_from(["anth-panel", "host-info", "--watch", interval]).unwrap();

        match cli.command {
            Command::HostInfo(args) => assert_eq!(args.watch.as_deref(), Some(interval)),
            other => panic!("expected HostInfo, got {other:?}"),
        }
    }
}

#[test]
fn cli_watch_flag_keeps_a_following_token_as_its_value() {
    // An optional value consumes the next token, so a malformed interval must
    // reach our parser rather than being silently dropped.
    let cli = Cli::try_parse_from(["anth-panel", "host-info", "--watch", "nope"]).unwrap();

    match cli.command {
        Command::HostInfo(args) => assert_eq!(args.watch.as_deref(), Some("nope")),
        other => panic!("expected HostInfo, got {other:?}"),
    }
}

#[test]
fn cli_other_subcommands_do_not_accept_watch() {
    assert!(Cli::try_parse_from(["anth-panel", "echo", "hi", "--watch"]).is_err());
    assert!(Cli::try_parse_from(["anth-panel", "daemon", "--watch"]).is_err());
}

#[test]
fn cli_parses_a_terminal_subcommand() {
    let cli = Cli::try_parse_from(["anth-panel", "terminal"]).unwrap();
    assert_eq!(cli.config_path, None);

    match cli.command {
        Command::Terminal(args) => {
            assert_eq!(args.cols, None);
            assert_eq!(args.rows, None);
        }
        other => panic!("expected Terminal, got {other:?}"),
    }
}

#[test]
fn cli_terminal_accepts_explicit_size_flags() {
    let cli =
        Cli::try_parse_from(["anth-panel", "terminal", "--cols", "120", "--rows", "40"]).unwrap();

    match cli.command {
        Command::Terminal(args) => {
            assert_eq!(args.cols, Some(120));
            assert_eq!(args.rows, Some(40));
        }
        other => panic!("expected Terminal, got {other:?}"),
    }
}
