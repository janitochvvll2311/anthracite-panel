use anth_cli::cli::{Cli, Command};
use anth_cli::{handle_response, request_for};
use anth_proto::{Request, Response};
use clap::Parser;

#[test]
fn request_for_echo_builds_an_echo_request() {
    let command = Command::Echo {
        message: "hello".to_string(),
    };

    match request_for(&command) {
        Request::Echo { message } => assert_eq!(message, "hello"),
    }
}

#[test]
fn cli_parses_an_echo_subcommand() {
    let cli = Cli::try_parse_from(["anth-cli", "echo", "hello"]).unwrap();
    assert_eq!(cli.config_path, None);

    match cli.command {
        Command::Echo { message } => assert_eq!(message, "hello"),
    }
}

#[test]
fn cli_accepts_a_config_flag_before_the_subcommand() {
    let cli =
        Cli::try_parse_from(["anth-cli", "--config", "/tmp/anth.toml", "echo", "hi"]).unwrap();
    assert_eq!(cli.config_path.as_deref(), Some("/tmp/anth.toml"));

    let cli = Cli::try_parse_from(["anth-cli", "-c", "/tmp/short.toml", "echo", "hi"]).unwrap();
    assert_eq!(cli.config_path.as_deref(), Some("/tmp/short.toml"));
}

#[test]
fn cli_requires_a_subcommand() {
    assert!(Cli::try_parse_from(["anth-cli"]).is_err());
}

#[test]
fn cli_rejects_an_echo_without_a_message() {
    assert!(Cli::try_parse_from(["anth-cli", "echo"]).is_err());
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
