use super::super::main_prelude::*;
use super::confirm::gate;
use super::server::{BackgroundServers, ServerRole};

#[test]
fn run_conflicts_with_both_services() {
    assert!(ServerRole::Run.serves(ServerRole::Daemon));
    assert!(ServerRole::Daemon.serves(ServerRole::Run));
    assert!(ServerRole::Run.serves(ServerRole::Api));
    assert!(ServerRole::Api.serves(ServerRole::Run));
    assert!(!ServerRole::Daemon.serves(ServerRole::Api));
    assert!(!ServerRole::Api.serves(ServerRole::Daemon));
}

#[tokio::test]
async fn a_daemon_leaves_the_api_free_but_blocks_a_second_daemon() {
    let mut servers = BackgroundServers::default();
    let role = servers.check(&Command::Daemon).unwrap();
    servers.register(role, tokio::spawn(std::future::pending::<()>()));

    assert!(
        servers
            .check(&Command::Api(EndpointArgs::default()))
            .is_ok(),
        "the API binds another port and must be allowed"
    );
    let error = servers.check(&Command::Daemon).unwrap_err();
    assert!(error.contains("already running"), "{error}");
    let error = servers
        .check(&Command::Run(EndpointArgs::default()))
        .unwrap_err();
    assert!(error.contains("cannot start `run`"), "{error}");
}

#[test]
fn a_declined_prompt_exits_zero_and_a_refusal_exits_one() {
    assert_eq!(gate(Ok(true)), None);
    assert_eq!(gate(Ok(false)), Some(ExitCode::SUCCESS));
    assert_eq!(
        gate(Err("no terminal".to_string())),
        Some(ExitCode::FAILURE)
    );
}
