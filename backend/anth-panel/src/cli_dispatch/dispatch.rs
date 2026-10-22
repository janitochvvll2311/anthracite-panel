use super::super::main_prelude::*;
use super::client::*;
use super::plugin_cmd::*;
use super::server::*;
use super::settings_cmd::*;

pub fn watch_interval(command: &Command) -> Result<Option<Duration>, String> {
    match command {
        Command::HostInfo(WatchArgs { watch }) => match watch.as_deref() {
            None => Ok(None),
            Some(raw) => stream::parse_interval(raw).map(Some),
        },
        Command::Process(ProcessArgs {
            command: ProcessCommand::List(WatchArgs { watch }),
        }) => match watch.as_deref() {
            None => Ok(None),
            Some(raw) => stream::parse_interval(raw).map(Some),
        },
        Command::Network(NetworkArgs {
            command: NetworkCommand::List(WatchArgs { watch }),
        }) => match watch.as_deref() {
            None => Ok(None),
            Some(raw) => stream::parse_interval(raw).map(Some),
        },
        _ => Ok(None),
    }
}

/// Runs one command to completion and reports its exit code.
///
/// This is the single dispatcher: `main` calls it once, and the interactive
/// session calls it per line, so a typed command cannot drift from the
/// one-shot invocation. `background` is `Some` only in a session; there a
/// serving subcommand is detached instead of awaited, which is what lets the
/// prompt keep sending commands while the daemon and the API run.
pub async fn dispatch(
    config_path: &str,
    command: &Command,
    background: Option<&RefCell<BackgroundServers>>,
) -> ExitCode {
    if let Some(servers) = background
        && ServerRole::of(command).is_some()
    {
        let role = match servers.borrow_mut().check(command) {
            Ok(role) => role,
            Err(error) => {
                eprintln!("[ERROR] {error}");
                return ExitCode::FAILURE;
            }
        };
        return match start_server(config_path, command).await {
            Ok(handle) => {
                servers.borrow_mut().register(role, handle);
                println!(
                    "[background] {} started; the prompt stays available, `quit` stops it",
                    role.label()
                );
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("[ERROR] {error}");
                ExitCode::FAILURE
            }
        };
    }

    if let Command::Auth(args) = command {
        return run_auth(config_path, args);
    }
    if let Command::Config(args) = command {
        return run_config(config_path, args);
    }
    if let Command::Plugin(args) = command {
        return run_plugin(config_path, args);
    }

    if let Some(code) = send_command(config_path, command).await {
        return code;
    }

    let result = match command {
        Command::Daemon => anth_panel::daemon::serve(
            &loaded_config(config_path),
            std::path::Path::new(config_path),
        )
        .await
        .map(|()| ExitCode::SUCCESS)
        .map_err(|e| e.to_string()),
        Command::Api(args) => serve_api(loaded_config(config_path), config_path, args, None).await,
        Command::Run(args) => serve_run(loaded_config(config_path), config_path, args, None).await,
        Command::Terminal(args) => {
            let config = Config::load_or_default(config_path);
            Ok(anth_panel::terminal_client::run(Path::new(&config.socket_path), args).await)
        }
        Command::Echo { .. }
        | Command::HostInfo(_)
        | Command::Process(_)
        | Command::Network(_)
        | Command::Firewall(_)
        | Command::Volume(_)
        | Command::Account(_)
        | Command::File(_)
        | Command::Auth(_)
        | Command::Config(_)
        | Command::Plugin(_) => {
            unreachable!("handled by send_command or run_auth/run_config")
        }
    };

    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("[Error] {e}");
            ExitCode::FAILURE
        }
    }
}

/// Runs the global interactive session, one command per line.
///
/// The runner borrows the config path and calls the same [`dispatch`] a
/// one-shot invocation uses; only the exit code is kept, because a session
/// continues after a failure. A serving subcommand is detached there, so the
/// registry is shared with the dispatcher and stopped when the prompt closes.
pub async fn run_interactive(config_path: &str, first: Option<Command>) -> ExitCode {
    let mut first = first;
    let path = config_path.to_string();
    let servers = Rc::new(RefCell::new(BackgroundServers::default()));
    let session_servers = Rc::clone(&servers);
    let mut runner: anth_panel::repl::Runner = Box::new(move |command| {
        let config_path = path.clone();
        let servers = Rc::clone(&session_servers);
        Box::pin(async move {
            let code = dispatch(&config_path, &command, Some(servers.as_ref())).await;
            u8::from(code == ExitCode::FAILURE)
        })
    });

    let stdin = tokio::io::stdin();
    let mut lines = tokio::io::BufReader::new(stdin).lines();
    let outcome = anth_panel::repl::session(first.take(), &mut lines, true, &mut runner).await;
    servers.borrow_mut().shutdown();

    if let Err(e) = outcome {
        eprintln!("[ERROR] {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
