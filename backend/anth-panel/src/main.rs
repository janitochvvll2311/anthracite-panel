use std::io::{Read, Write};
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use anth_panel::cli::{
    AccountArgs, AccountCommand, AccountGroupCommand, AccountGroupDeleteArgs,
    AccountGroupModifyArgs, AccountUserArgs, AccountUserCommand, AccountUserDeleteArgs,
    AccountUserPasswordArgs, AuthArgs, AuthCommand, Cli, Command, ConfigArgs, ConfigCommand,
    EndpointArgs, FileArgs, FileCommand, FileDeleteArgs, FileReadArgs, FileUploadArgs,
    FirewallAddArgs, FirewallArgs, FirewallCommand, FirewallDeleteArgs, FirewallFlushArgs,
    NetworkArgs, NetworkCommand, NetworkStateArgs, ProcessArgs, ProcessCommand, ProcessSignalArgs,
    ProcessSpawnArgs, VolumeArgs, VolumeCommand, VolumeMountArgs, VolumeUnmountArgs, WatchArgs,
};
use anth_panel::config::{Config, DEFAULT_CONFIG_PATH};
use anth_panel::read_password;
use anth_panel::stream;
use anth_panel::{AppState, request_for, router};
use anth_proto::{AnthClient, DEFAULT_FILE_CHUNK, FileChunk, FirewallAction, Request, Response};
use tokio::net::TcpListener;

const DEFAULT_ADDR: &str = "0.0.0.0:8080";

fn resolve_addr(args: &EndpointArgs) -> String {
    args.addr
        .clone()
        .or_else(|| std::env::var("ANTH_PANEL_ADDR").ok())
        .unwrap_or_else(|| DEFAULT_ADDR.to_string())
}

async fn serve_api(
    config: Config,
    config_path: &str,
    args: &EndpointArgs,
) -> Result<ExitCode, String> {
    let address = resolve_addr(args);
    let listener = TcpListener::bind(&address)
        .await
        .map_err(|e| format!("Failed to bind to address {address}: {e}"))?;

    println!("Server is listening on {address}");
    let state = AppState::with_config_path(config, Path::new(config_path));
    axum::serve(listener, router(state))
        .await
        .map_err(|e| format!("Server error: {e}"))?;
    Ok(ExitCode::SUCCESS)
}

/// Keeps the daemon task alive for as long as the API runs: binding the HTTP
/// port first surfaces address-in-use errors before the socket is touched.
async fn serve_run(
    config: Config,
    config_path: &str,
    args: &EndpointArgs,
) -> Result<ExitCode, String> {
    let address = resolve_addr(args);
    let listener = TcpListener::bind(&address)
        .await
        .map_err(|e| format!("Failed to bind to address {address}: {e}"))?;

    let daemon_config = config.clone();
    let daemon_config_path = std::path::PathBuf::from(config_path);
    let daemon = tokio::spawn(async move {
        if let Err(e) = anth_panel::daemon::serve(&daemon_config, &daemon_config_path).await {
            eprintln!("Daemon stopped: {e}");
        }
    });

    println!("Daemon and server are running (http://{address})");
    let state = AppState::with_config_path(config, Path::new(config_path));
    let result = axum::serve(listener, router(state))
        .await
        .map_err(|e| format!("Server error: {e}"));

    daemon.abort();
    let _ = daemon.await;
    result.map(|_| ExitCode::SUCCESS)
}

/// Sends `account user password`. The password is read here, never assembled
/// from argv, and travels only in the request body.
async fn send_account_password(config_path: &str, args: &AccountUserPasswordArgs) -> ExitCode {
    if let Err(error) = confirm_account_password(args) {
        return fail_confirmation(error);
    }
    let password = match read_password(args.stdin) {
        Ok(password) => password,
        Err(error) => {
            eprintln!("[ERROR] {error}");
            return ExitCode::FAILURE;
        }
    };

    let request = Request::AccountSetPassword {
        name: args.name.clone(),
        password,
    };
    let config = Config::load_or_default(config_path);
    let socket = match tokio::net::UnixStream::connect(&config.socket_path).await {
        Ok(socket) => socket,
        Err(error) => {
            eprintln!("[ERROR] could not reach the daemon: {error}");
            return ExitCode::FAILURE;
        }
    };
    let mut client = AnthClient::new(socket);
    let response = match stream::read_once(&mut client, &request).await {
        Ok(response) => response,
        Err(error) => {
            eprintln!("[ERROR] {error}");
            return ExitCode::FAILURE;
        }
    };
    match response {
        Response::AccountChanged { message } => {
            println!("[OK] {message}");
            ExitCode::SUCCESS
        }
        Response::AccountError(error) => {
            eprintln!("[ERROR] {}", error.detail);
            ExitCode::FAILURE
        }
        Response::Error { detail } => {
            eprintln!("[ERROR] {detail}");
            ExitCode::FAILURE
        }
        other => {
            eprintln!("[ERROR] unexpected response: {other:?}");
            ExitCode::FAILURE
        }
    }
}

fn fail_confirmation(error: String) -> ExitCode {
    eprintln!("[ERROR] {error}");
    ExitCode::FAILURE
}

/// Prints the cancellation notice once, for every account subcommand that asks.
fn report_cancel(confirmed: bool) -> bool {
    if !confirmed {
        eprintln!("[cancelled]");
    }
    confirmed
}

fn confirm_account_password(args: &AccountUserPasswordArgs) -> Result<bool, String> {
    confirm_action(
        args.yes,
        || format!("Set the password for `{}`? [y/N] ", args.name),
        "refusing to change a password without --yes when stdin is not a terminal",
    )
    .map(report_cancel)
}

fn confirm_account_user_delete(args: &AccountUserDeleteArgs) -> Result<bool, String> {
    let extra = if args.remove_home {
        " and delete its home directory"
    } else {
        ""
    };
    confirm_action(
        args.yes,
        || format!("Delete user `{}`{extra}? [y/N] ", args.name),
        "refusing to delete a user without --yes when stdin is not a terminal",
    )
    .map(report_cancel)
}

fn confirm_account_group_delete(args: &AccountGroupDeleteArgs) -> Result<bool, String> {
    confirm_action(
        args.yes,
        || format!("Delete group `{}`? [y/N] ", args.name),
        "refusing to delete a group without --yes when stdin is not a terminal",
    )
    .map(report_cancel)
}

fn confirm_account_group_modify(args: &AccountGroupModifyArgs) -> Result<bool, String> {
    confirm_action(
        args.yes,
        || format!("Replace the members of `{}`? [y/N] ", args.name),
        "refusing to change group membership without --yes when stdin is not a terminal",
    )
    .map(report_cancel)
}

/// Confirmation for every account subcommand that changes or removes data.
/// `None` covers the read-only and additive ones, which need no prompt.
fn account_confirmation(command: &Command) -> Option<Result<bool, String>> {
    let Command::Account(AccountArgs { command }) = command else {
        return None;
    };
    match command {
        AccountCommand::List => None,
        AccountCommand::User(args) => match &args.command {
            AccountUserCommand::Create(_) => None,
            AccountUserCommand::Modify(_) => None,
            AccountUserCommand::Delete(args) => Some(confirm_account_user_delete(args)),
            // Handled by `send_account_password` before this runs.
            AccountUserCommand::Password(_) => None,
        },
        AccountCommand::Group(args) => match &args.command {
            AccountGroupCommand::Create(_) => None,
            AccountGroupCommand::Modify(args) if args.members.is_some() => {
                Some(confirm_account_group_modify(args))
            }
            AccountGroupCommand::Modify(_) => None,
            AccountGroupCommand::Delete(args) => Some(confirm_account_group_delete(args)),
        },
    }
}

fn watch_interval(command: &Command) -> Result<Option<Duration>, String> {
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

fn confirm_action(
    yes: bool,
    prompt: impl FnOnce() -> String,
    refusal: &str,
) -> Result<bool, String> {
    use std::io::{IsTerminal, Write};

    if yes {
        return Ok(true);
    }

    let stdin = std::io::stdin();
    if !stdin.is_terminal() {
        return Err(refusal.to_string());
    }

    print!("{}", prompt());
    let _ = std::io::stdout().flush();

    let mut answer = String::new();
    stdin
        .read_line(&mut answer)
        .map_err(|error| format!("failed to read confirmation: {error}"))?;

    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

fn confirm_signal(args: &ProcessSignalArgs) -> Result<bool, String> {
    confirm_action(
        args.yes,
        || format!("Send {} to process {}? [y/N] ", args.signal, args.pid),
        "refusing to signal without --yes when stdin is not a terminal",
    )
}

fn confirm_spawn(args: &ProcessSpawnArgs) -> Result<bool, String> {
    let command = if args.args.is_empty() {
        args.program.clone()
    } else {
        format!("{} {}", args.program, args.args.join(" "))
    };
    confirm_action(
        args.yes,
        || format!("Start `{command}`? [y/N] "),
        "refusing to start a process without --yes when stdin is not a terminal",
    )
}

fn confirm_network_state(args: &NetworkStateArgs, up: bool) -> Result<bool, String> {
    let action = if up { "Bring up" } else { "Bring down" };
    confirm_action(
        args.yes,
        || {
            format!(
                "{action} interface `{}`? This can disconnect the host. [y/N] ",
                args.name
            )
        },
        "refusing to change a network interface without --yes when stdin is not a terminal",
    )
}

fn confirm_firewall_add(args: &FirewallAddArgs, action: FirewallAction) -> Result<bool, String> {
    let verb = if action == FirewallAction::Allow {
        "Allow"
    } else {
        "Deny"
    };
    confirm_action(
        args.yes,
        || {
            format!(
                "{verb} {}/{} traffic? [y/N] ",
                args.spec.protocol, args.spec.port
            )
        },
        "refusing to change the firewall without --yes when stdin is not a terminal",
    )
}

fn confirm_firewall_delete(args: &FirewallDeleteArgs) -> Result<bool, String> {
    confirm_action(
        args.yes,
        || format!("Delete firewall rule {}? [y/N] ", args.handle),
        "refusing to change the firewall without --yes when stdin is not a terminal",
    )
}

fn confirm_firewall_flush(args: &FirewallFlushArgs) -> Result<bool, String> {
    confirm_action(
        args.yes,
        || "Flush every rule from the managed firewall chain? [y/N] ".to_string(),
        "refusing to change the firewall without --yes when stdin is not a terminal",
    )
}

fn confirm_delete(args: &FileDeleteArgs) -> Result<bool, String> {
    let verb = if args.recursive {
        "Recursively delete"
    } else {
        "Delete"
    };
    confirm_action(
        args.yes,
        || format!("{verb} `{}`? [y/N] ", args.path),
        "refusing to delete without --yes when stdin is not a terminal",
    )
}

fn confirm_volume_mount(args: &VolumeMountArgs) -> Result<bool, String> {
    confirm_action(
        args.yes,
        || {
            let mode = if args.read_only { " read-only" } else { "" };
            format!(
                "Mount `{}` on `{}`{}? [y/N] ",
                args.device, args.target, mode
            )
        },
        "refusing to mount without --yes when stdin is not a terminal",
    )
}

fn confirm_volume_unmount(args: &VolumeUnmountArgs) -> Result<bool, String> {
    confirm_action(
        args.yes,
        || format!("Unmount `{}`? [y/N] ", args.target),
        "refusing to unmount without --yes when stdin is not a terminal",
    )
}

fn client_config(config_path: &str) -> Config {
    Config::load_or_default(config_path)
}

async fn connect_client(config: &Config) -> Result<AnthClient, ExitCode> {
    match tokio::net::UnixStream::connect(Path::new(&config.socket_path)).await {
        Ok(stream) => Ok(AnthClient::new(stream)),
        Err(e) => {
            eprintln!("Failed to connect to socket: {e}");
            Err(ExitCode::FAILURE)
        }
    }
}

async fn exchange(client: &mut AnthClient, request: &Request) -> Result<Response, String> {
    client
        .send_request(request)
        .await
        .map_err(|e| format!("Failed to send request: {e}"))?;
    match client.receive_response().await {
        Some(Ok(response)) => Ok(response),
        Some(Err(e)) => Err(format!("Failed to receive response: {e}")),
        None => Err("Stream closed while waiting for response".to_string()),
    }
}

async fn send_file_read(config_path: &str, args: &FileReadArgs) -> ExitCode {
    let config = client_config(config_path);
    let mut client = match connect_client(&config).await {
        Ok(client) => client,
        Err(code) => return code,
    };

    let mut output: Box<dyn Write> = match &args.output {
        Some(path) => match std::fs::File::create(path) {
            Ok(file) => Box::new(file),
            Err(e) => {
                eprintln!("[ERROR] cannot create {path}: {e}");
                return ExitCode::FAILURE;
            }
        },
        None => Box::new(std::io::stdout()),
    };

    let mut offset = 0u64;
    loop {
        let request = Request::FileRead {
            path: args.path.clone(),
            offset,
            len: DEFAULT_FILE_CHUNK,
        };
        let response = match exchange(&mut client, &request).await {
            Ok(response) => response,
            Err(error) => {
                eprintln!("[ERROR] {error}");
                return ExitCode::FAILURE;
            }
        };

        match response {
            Response::FileChunk(FileChunk {
                offset: chunk_offset,
                bytes,
                eof,
                ..
            }) => {
                if let Err(e) = output.write_all(&bytes) {
                    eprintln!("[ERROR] cannot write output: {e}");
                    return ExitCode::FAILURE;
                }
                if eof {
                    break;
                }
                if bytes.is_empty() {
                    eprintln!("[ERROR] daemon returned an empty chunk before EOF");
                    return ExitCode::FAILURE;
                }
                offset = chunk_offset + bytes.len() as u64;
            }
            Response::FileError(error) => {
                eprintln!("[ERROR] {}", error.detail);
                return ExitCode::FAILURE;
            }
            Response::Error { detail } => {
                eprintln!("[ERROR] {detail}");
                return ExitCode::FAILURE;
            }
            other => {
                eprintln!("[ERROR] unexpected daemon response: {other:?}");
                return ExitCode::FAILURE;
            }
        }
    }

    if let Err(e) = output.flush() {
        eprintln!("[ERROR] cannot flush output: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

async fn send_file_upload(config_path: &str, args: &FileUploadArgs) -> ExitCode {
    let mut local = match std::fs::File::open(&args.local) {
        Ok(file) => file,
        Err(e) => {
            eprintln!("[ERROR] cannot open {}: {e}", args.local);
            return ExitCode::FAILURE;
        }
    };
    let config = client_config(config_path);
    let mut client = match connect_client(&config).await {
        Ok(client) => client,
        Err(code) => return code,
    };

    let id = match exchange(
        &mut client,
        &Request::FileWriteStart {
            path: args.dest.clone(),
            overwrite: args.overwrite,
        },
    )
    .await
    {
        Ok(Response::FileWriteStarted { id }) => id,
        Ok(Response::FileError(error)) => {
            eprintln!("[ERROR] {}", error.detail);
            return ExitCode::FAILURE;
        }
        Ok(Response::Error { detail }) => {
            eprintln!("[ERROR] {detail}");
            return ExitCode::FAILURE;
        }
        Ok(other) => {
            eprintln!("[ERROR] unexpected daemon response: {other:?}");
            return ExitCode::FAILURE;
        }
        Err(error) => {
            eprintln!("[ERROR] {error}");
            return ExitCode::FAILURE;
        }
    };

    let mut buffer = vec![0u8; DEFAULT_FILE_CHUNK as usize];
    loop {
        let read = match local.read(&mut buffer) {
            Ok(read) => read,
            Err(e) => {
                eprintln!("[ERROR] cannot read {}: {e}", args.local);
                let _ = exchange(&mut client, &Request::FileWriteAbort { id }).await;
                return ExitCode::FAILURE;
            }
        };
        if read == 0 {
            break;
        }

        match exchange(
            &mut client,
            &Request::FileWriteChunk {
                id,
                bytes: buffer[..read].to_vec(),
            },
        )
        .await
        {
            Ok(Response::FileWriteAccepted { .. }) => {}
            Ok(Response::FileError(error)) => {
                eprintln!("[ERROR] {}", error.detail);
                let _ = exchange(&mut client, &Request::FileWriteAbort { id }).await;
                return ExitCode::FAILURE;
            }
            Ok(Response::Error { detail }) => {
                eprintln!("[ERROR] {detail}");
                let _ = exchange(&mut client, &Request::FileWriteAbort { id }).await;
                return ExitCode::FAILURE;
            }
            Ok(other) => {
                eprintln!("[ERROR] unexpected daemon response: {other:?}");
                let _ = exchange(&mut client, &Request::FileWriteAbort { id }).await;
                return ExitCode::FAILURE;
            }
            Err(error) => {
                eprintln!("[ERROR] {error}");
                return ExitCode::FAILURE;
            }
        }
    }

    match exchange(&mut client, &Request::FileWriteFinish { id }).await {
        Ok(Response::FileWritten(written)) => {
            println!("[OK] wrote {} ({} bytes)", written.path, written.size);
            ExitCode::SUCCESS
        }
        Ok(Response::FileError(error)) => {
            eprintln!("[ERROR] {}", error.detail);
            ExitCode::FAILURE
        }
        Ok(Response::Error { detail }) => {
            eprintln!("[ERROR] {detail}");
            ExitCode::FAILURE
        }
        Ok(other) => {
            eprintln!("[ERROR] unexpected daemon response: {other:?}");
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("[ERROR] {error}");
            ExitCode::FAILURE
        }
    }
}

/// Runs a client-only subcommand against the daemon. Subcommands that serve
/// locally return `None` so `main` can handle them itself.
async fn send_command(config_path: &str, command: &Command) -> Option<ExitCode> {
    if let Command::File(FileArgs {
        command: FileCommand::Read(args),
    }) = command
    {
        return Some(send_file_read(config_path, args).await);
    }

    if let Command::File(FileArgs {
        command: FileCommand::Upload(args),
    }) = command
    {
        return Some(send_file_upload(config_path, args).await);
    }

    if let Command::Process(ProcessArgs {
        command: ProcessCommand::Spawn(args),
    }) = command
    {
        match confirm_spawn(args) {
            Ok(true) => {}
            Ok(false) => {
                eprintln!("[cancelled]");
                return Some(ExitCode::SUCCESS);
            }
            Err(error) => {
                eprintln!("[ERROR] {error}");
                return Some(ExitCode::FAILURE);
            }
        }
    }

    if let Command::Process(ProcessArgs {
        command: ProcessCommand::Signal(args),
    }) = command
    {
        match confirm_signal(args) {
            Ok(true) => {}
            Ok(false) => {
                eprintln!("[cancelled]");
                return Some(ExitCode::SUCCESS);
            }
            Err(error) => {
                eprintln!("[ERROR] {error}");
                return Some(ExitCode::FAILURE);
            }
        }
    }

    if let Command::Network(NetworkArgs {
        command: NetworkCommand::Up(args),
    }) = command
    {
        match confirm_network_state(args, true) {
            Ok(true) => {}
            Ok(false) => {
                eprintln!("[cancelled]");
                return Some(ExitCode::SUCCESS);
            }
            Err(error) => {
                eprintln!("[ERROR] {error}");
                return Some(ExitCode::FAILURE);
            }
        }
    }

    if let Command::Network(NetworkArgs {
        command: NetworkCommand::Down(args),
    }) = command
    {
        match confirm_network_state(args, false) {
            Ok(true) => {}
            Ok(false) => {
                eprintln!("[cancelled]");
                return Some(ExitCode::SUCCESS);
            }
            Err(error) => {
                eprintln!("[ERROR] {error}");
                return Some(ExitCode::FAILURE);
            }
        }
    }

    if let Command::Volume(VolumeArgs {
        command: VolumeCommand::Mount(args),
    }) = command
    {
        match confirm_volume_mount(args) {
            Ok(true) => {}
            Ok(false) => {
                eprintln!("[cancelled]");
                return Some(ExitCode::SUCCESS);
            }
            Err(error) => {
                eprintln!("[ERROR] {error}");
                return Some(ExitCode::FAILURE);
            }
        }
    }

    if let Command::Volume(VolumeArgs {
        command: VolumeCommand::Unmount(args),
    }) = command
    {
        match confirm_volume_unmount(args) {
            Ok(true) => {}
            Ok(false) => {
                eprintln!("[cancelled]");
                return Some(ExitCode::SUCCESS);
            }
            Err(error) => {
                eprintln!("[ERROR] {error}");
                return Some(ExitCode::FAILURE);
            }
        }
    }

    if let Command::Firewall(FirewallArgs {
        command: FirewallCommand::Allow(args),
    }) = command
    {
        match confirm_firewall_add(args, FirewallAction::Allow) {
            Ok(true) => {}
            Ok(false) => {
                eprintln!("[cancelled]");
                return Some(ExitCode::SUCCESS);
            }
            Err(error) => {
                eprintln!("[ERROR] {error}");
                return Some(ExitCode::FAILURE);
            }
        }
    }

    if let Command::Firewall(FirewallArgs {
        command: FirewallCommand::Deny(args),
    }) = command
    {
        match confirm_firewall_add(args, FirewallAction::Deny) {
            Ok(true) => {}
            Ok(false) => {
                eprintln!("[cancelled]");
                return Some(ExitCode::SUCCESS);
            }
            Err(error) => {
                eprintln!("[ERROR] {error}");
                return Some(ExitCode::FAILURE);
            }
        }
    }

    if let Command::Firewall(FirewallArgs {
        command: FirewallCommand::Delete(args),
    }) = command
    {
        match confirm_firewall_delete(args) {
            Ok(true) => {}
            Ok(false) => {
                eprintln!("[cancelled]");
                return Some(ExitCode::SUCCESS);
            }
            Err(error) => {
                eprintln!("[ERROR] {error}");
                return Some(ExitCode::FAILURE);
            }
        }
    }

    if let Command::Firewall(FirewallArgs {
        command: FirewallCommand::Flush(args),
    }) = command
    {
        match confirm_firewall_flush(args) {
            Ok(true) => {}
            Ok(false) => {
                eprintln!("[cancelled]");
                return Some(ExitCode::SUCCESS);
            }
            Err(error) => {
                eprintln!("[ERROR] {error}");
                return Some(ExitCode::FAILURE);
            }
        }
    }

    if let Command::File(FileArgs {
        command: FileCommand::Delete(args),
    }) = command
    {
        match confirm_delete(args) {
            Ok(true) => {}
            Ok(false) => {
                eprintln!("[cancelled]");
                return Some(ExitCode::SUCCESS);
            }
            Err(error) => {
                eprintln!("[ERROR] {error}");
                return Some(ExitCode::FAILURE);
            }
        }
    }

    // The password never travels through `request_for`: it is read from the
    // terminal here and sent on its own request.
    if let Command::Account(AccountArgs {
        command:
            AccountCommand::User(AccountUserArgs {
                command: AccountUserCommand::Password(args),
            }),
    }) = command
    {
        return Some(send_account_password(config_path, args).await);
    }

    for confirmed in [account_confirmation(command)].into_iter().flatten() {
        match confirmed {
            Ok(true) => {}
            Ok(false) => return Some(ExitCode::SUCCESS),
            Err(error) => {
                eprintln!("[ERROR] {error}");
                return Some(ExitCode::FAILURE);
            }
        }
    }

    let request: Request = request_for(command)?;
    let interval = match watch_interval(command) {
        Ok(interval) => interval,
        Err(e) => {
            eprintln!("[ERROR] {e}");
            return Some(ExitCode::FAILURE);
        }
    };

    // Client-only subcommands never bootstrap the config file.
    let config = Config::load_or_default(config_path);
    Some(
        stream::follow(
            Path::new(&config.socket_path),
            &request,
            interval,
            anth_panel::render_response,
        )
        .await,
    )
}

/// Runs one `auth` subcommand against the config file.
fn run_auth(config_path: &str, args: &AuthArgs) -> ExitCode {
    let config = Config::load_or_create(config_path);

    let updated = match &args.command {
        AuthCommand::Status => {
            anth_panel::settings::report_status(&config);
            return ExitCode::SUCCESS;
        }
        AuthCommand::Enable(toggle) => anth_panel::settings::toggle_auth(config, toggle),
        AuthCommand::SetPassword(password_args) => {
            let password = match anth_panel::settings::read_password(password_args) {
                Ok(password) => password,
                Err(error) => {
                    eprintln!("[ERROR] {error}");
                    return ExitCode::FAILURE;
                }
            };
            anth_panel::settings::set_password(config, password.expose())
        }
    };

    match updated {
        Ok(config) => match config.save(config_path) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("[ERROR] {error}");
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            eprintln!("[ERROR] {error}");
            ExitCode::FAILURE
        }
    }
}

/// Runs one `config` subcommand against the config file.
fn run_config(config_path: &str, args: &ConfigArgs) -> ExitCode {
    match &args.command {
        ConfigCommand::Get => {
            let config = Config::load_or_create(config_path);
            match config.to_toml() {
                Ok(text) => {
                    print!("{text}");
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("[ERROR] {error}");
                    ExitCode::FAILURE
                }
            }
        }
        ConfigCommand::Set(set) => {
            let mut config = Config::load_or_create(config_path);
            if let Err(error) = anth_panel::settings::set_key(&mut config, set) {
                eprintln!("[ERROR] {error}");
                return ExitCode::FAILURE;
            }
            match config.save(config_path) {
                Ok(()) => {
                    println!("{} = {}", set.key, set.value);
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("[ERROR] {error}");
                    ExitCode::FAILURE
                }
            }
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let config_path = cli.config_path.as_deref().unwrap_or(DEFAULT_CONFIG_PATH);

    if let Command::Auth(args) = &cli.command {
        return run_auth(config_path, args);
    }
    if let Command::Config(args) = &cli.command {
        return run_config(config_path, args);
    }

    if let Some(code) = send_command(config_path, &cli.command).await {
        return code;
    }

    let result = match &cli.command {
        Command::Daemon => anth_panel::daemon::serve(
            &Config::load_or_create(config_path),
            std::path::Path::new(config_path),
        )
        .await
        .map(|()| ExitCode::SUCCESS)
        .map_err(|e| e.to_string()),
        Command::Api(args) => {
            serve_api(Config::load_or_create(config_path), config_path, args).await
        }
        Command::Run(args) => {
            serve_run(Config::load_or_create(config_path), config_path, args).await
        }
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
        | Command::Config(_) => {
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
