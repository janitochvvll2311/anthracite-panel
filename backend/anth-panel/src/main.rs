use std::cell::RefCell;
use std::io::{Read, Write};
use std::path::Path;
use std::process::ExitCode;
use std::rc::Rc;
use std::time::Duration;

use anth_panel::cli::{
    AccountArgs, AccountCommand, AccountGroupCommand, AccountGroupDeleteArgs,
    AccountGroupModifyArgs, AccountUserArgs, AccountUserCommand, AccountUserDeleteArgs,
    AccountUserPasswordArgs, AuthArgs, AuthCommand, Cli, Command, ConfigArgs, ConfigCommand,
    EndpointArgs, FileArgs, FileCommand, FileDeleteArgs, FileReadArgs, FileUploadArgs,
    FileWriteArgs, FirewallAddArgs, FirewallArgs, FirewallCommand, FirewallDeleteArgs,
    FirewallFlushArgs, NetworkArgs, NetworkCommand, NetworkKindArg, NetworkStateArgs, PluginArgs,
    PluginCommand, ProcessArgs, ProcessCommand, ProcessSignalArgs, ProcessSpawnArgs,
    TrashBinCommand, TrashEmptyArgs, TrashRestoreArgs, VolumeArgs, VolumeCommand, VolumeMountArgs,
    VolumeUnmountArgs, WatchArgs,
};
use anth_panel::config::{Config, DEFAULT_CONFIG_PATH};
use anth_panel::read_password;
use anth_panel::stream;
use anth_panel::{AppState, request_for, router};
use anth_proto::{AnthClient, DEFAULT_FILE_CHUNK, FileChunk, FirewallAction, Request, Response};
use std::net::SocketAddr;
use tokio::io::AsyncBufReadExt;
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
    ready: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
) -> Result<ExitCode, String> {
    let address = resolve_addr(args);
    let listener = match TcpListener::bind(&address).await {
        Ok(listener) => listener,
        Err(error) => {
            let detail = format!("Failed to bind to address {address}: {error}");
            notify_ready(ready, Err(detail.clone()));
            return Err(detail);
        }
    };

    println!("Server is listening on {address}");
    notify_ready(ready, Ok(()));
    let state = AppState::with_config_path(config, Path::new(config_path));
    axum::serve(
        listener,
        router(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .map_err(|e| format!("Server error: {e}"))?;
    Ok(ExitCode::SUCCESS)
}

/// Hands the bind outcome to a waiting [`dispatch`] without making the serving
/// functions depend on it. A dropped receiver is not an error: a one-shot
/// invocation passes `None`, and the interactive session is the only caller
/// that waits.
fn notify_ready(
    ready: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
    outcome: Result<(), String>,
) {
    if let Some(ready) = ready {
        let _ = ready.send(outcome);
    }
}

/// Keeps the daemon task alive for as long as the API runs: binding the HTTP
/// port first surfaces address-in-use errors before the socket is touched.
async fn serve_run(
    config: Config,
    config_path: &str,
    args: &EndpointArgs,
    ready: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
) -> Result<ExitCode, String> {
    let address = resolve_addr(args);
    let listener = match TcpListener::bind(&address).await {
        Ok(listener) => listener,
        Err(error) => {
            let detail = format!("Failed to bind to address {address}: {error}");
            notify_ready(ready, Err(detail.clone()));
            return Err(detail);
        }
    };

    let daemon_config = config.clone();
    let daemon_config_path = std::path::PathBuf::from(config_path);
    let (daemon_ready, daemon_bound) = tokio::sync::oneshot::channel();
    let daemon = tokio::spawn(async move {
        if let Err(e) = anth_panel::daemon::serve_with_ready(
            &daemon_config,
            &daemon_config_path,
            Some(daemon_ready),
        )
        .await
        {
            eprintln!("Daemon stopped: {e}");
        }
    });

    // The HTTP port is bound, but `run` is not ready until the socket is too:
    // an interactive session sends its first request as soon as this returns.
    match daemon_bound.await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            daemon.abort();
            let _ = daemon.await;
            notify_ready(ready, Err(error.clone()));
            return Err(error);
        }
        Err(_) => {
            daemon.abort();
            let _ = daemon.await;
            let detail = "the daemon stopped before binding the socket".to_string();
            notify_ready(ready, Err(detail.clone()));
            return Err(detail);
        }
    }

    println!("Daemon and server are running (http://{address})");
    notify_ready(ready, Ok(()));
    let state = AppState::with_config_path(config, Path::new(config_path));
    let result = axum::serve(
        listener,
        router(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
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

/// Asks before a create, edit, rename or delete.
///
/// Delete gets the harshest prompt: it names the interface, says whether it is
/// physical, and (for a physical device) says the host will lose that link. The
/// guard that stops a delete of the connection itself lives in the daemon,
/// because only the daemon can resolve the peer.
fn confirm_network_manage(command: &NetworkCommand) -> Option<ExitCode> {
    let outcome = match command {
        NetworkCommand::Create(args) => confirm_action(
            args.yes,
            || {
                format!(
                    "Create {} interface `{}`? [y/N] ",
                    kind_name(args.kind),
                    args.name
                )
            },
            "refusing to create an interface without --yes when stdin is not a terminal",
        ),
        NetworkCommand::Edit(args) => {
            let change = match (args.mtu, args.up, args.down) {
                (Some(mtu), true, _) => format!("set MTU to {mtu} and bring it up"),
                (Some(mtu), _, true) => format!("set MTU to {mtu} and bring it down"),
                (Some(mtu), _, _) => format!("set MTU to {mtu}"),
                (None, true, _) => "bring it up".to_string(),
                (None, _, true) => "bring it down".to_string(),
                (None, false, false) => "change nothing".to_string(),
            };
            confirm_action(
                args.yes,
                || format!("{change} on interface `{}`? [y/N] ", args.name),
                "refusing to edit an interface without --yes when stdin is not a terminal",
            )
        }
        NetworkCommand::Rename(args) => confirm_action(
            args.yes,
            || format!("Rename `{}` to `{}`? [y/N] ", args.name, args.new_name),
            "refusing to rename an interface without --yes when stdin is not a terminal",
        ),
        NetworkCommand::Delete(args) => confirm_action(
            args.yes,
            || {
                format!(
                    "Delete interface `{}`? This cannot be undone and may disconnect the host. [y/N] ",
                    args.name
                )
            },
            "refusing to delete an interface without --yes when stdin is not a terminal",
        ),
        NetworkCommand::List(_) | NetworkCommand::Up(_) | NetworkCommand::Down(_) => {
            return None;
        }
    };

    match outcome {
        Ok(true) => None,
        Ok(false) => {
            eprintln!("[cancelled]");
            Some(ExitCode::SUCCESS)
        }
        Err(error) => {
            eprintln!("[ERROR] {error}");
            Some(ExitCode::FAILURE)
        }
    }
}

fn kind_name(kind: NetworkKindArg) -> &'static str {
    match kind {
        NetworkKindArg::Veth => "veth",
        NetworkKindArg::Bridge => "bridge",
        NetworkKindArg::Bond => "bond",
        NetworkKindArg::Vlan => "vlan",
        NetworkKindArg::Dummy => "dummy",
        NetworkKindArg::TunTap => "tun/tap",
    }
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

fn confirm_trash_restore(args: &TrashRestoreArgs) -> Result<bool, String> {
    let verb = if args.overwrite {
        "Restore over the existing entry"
    } else {
        "Restore"
    };
    confirm_action(
        args.yes,
        || format!("{verb} `{}`? [y/N] ", args.id),
        "refusing to restore without --yes when stdin is not a terminal",
    )
}

fn confirm_trash_empty(args: &TrashEmptyArgs) -> Result<bool, String> {
    confirm_action(
        args.yes,
        || "Empty the trash? This cannot be undone. [y/N] ".to_string(),
        "refusing to empty the trash without --yes when stdin is not a terminal",
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
            require_existing: false,
            expected_modified_ms: None,
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

    if let Command::Network(NetworkArgs { command }) = command
        && let Some(response) = confirm_network_manage(command)
    {
        return Some(response);
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

    // Restoring over an existing entry and emptying the trash are the two
    // irreversible trash operations, so they confirm like a delete does.
    if let Command::File(FileArgs {
        command: FileCommand::TrashBin(args),
    }) = command
    {
        let confirmed = match &args.command {
            TrashBinCommand::Restore(args) => confirm_trash_restore(args),
            TrashBinCommand::Empty(args) => confirm_trash_empty(args),
            TrashBinCommand::List => Ok(true),
        };
        match confirmed {
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

    if let Command::File(FileArgs {
        command: FileCommand::Write(args),
    }) = command
    {
        return Some(write_text_file(config_path, args).await);
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

/// Replaces a remote text file with a local one, or with stdin.
///
/// The body travels as chunks for the same reason an upload does: a single
/// socket line is capped at 1 MiB, and an editor that can open a file must be
/// able to save it. `--if-unchanged` reuses the editor's optimistic
/// precondition, so a script cannot clobber a file that moved under it.
async fn write_text_file(config_path: &str, args: &FileWriteArgs) -> ExitCode {
    let mut contents = String::new();
    let read = if args.source == "-" {
        std::io::stdin().read_to_string(&mut contents)
    } else {
        std::fs::read_to_string(&args.source).map(|text| {
            contents = text;
            0
        })
    };
    if let Err(error) = read {
        eprintln!("[ERROR] cannot read `{}`: {error}", args.source);
        return ExitCode::FAILURE;
    }

    let config = client_config(config_path);
    let mut client = match connect_client(&config).await {
        Ok(client) => client,
        Err(code) => return code,
    };

    let id = match exchange(
        &mut client,
        &Request::FileWriteStart {
            path: args.path.clone(),
            overwrite: true,
            require_existing: true,
            expected_modified_ms: args.expected_modified_ms,
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

    for chunk in contents.as_bytes().chunks(DEFAULT_FILE_CHUNK as usize) {
        match exchange(
            &mut client,
            &Request::FileWriteChunk {
                id,
                bytes: chunk.to_vec(),
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

/// `plugin` acts on the bundle directory and the grants in the config, so it
/// runs locally, like `auth` and `config`. A running server reads the grants at
/// start, and the command says so when it changes anything.
fn run_plugin(config_path: &str, args: &PluginArgs) -> ExitCode {
    let mut config = Config::load_or_create(config_path);
    let plugins = config.plugins.clone();

    match &args.command {
        PluginCommand::List => {
            let (installed, problems) =
                anth_panel::plugins::bundle::list(&plugins.root_path(), &plugins.grants);
            print!(
                "{}",
                anth_panel::render::render_plugin_listing(&installed, &problems)
            );
            if !problems.is_empty() && installed.is_empty() {
                return ExitCode::FAILURE;
            }
            ExitCode::SUCCESS
        }
        PluginCommand::Install(install) => {
            let path = std::path::Path::new(&install.bundle);
            let (installed, grant) = match anth_panel::plugins::install_bundle(path, &plugins) {
                Ok(result) => result,
                Err(error) => {
                    eprintln!("[ERROR] {error}");
                    return ExitCode::FAILURE;
                }
            };
            println!(
                "Installed {} {} with {} capabilit{}",
                installed.plugin.id,
                installed.plugin.version,
                grant.capabilities.len(),
                if grant.capabilities.len() == 1 {
                    "y"
                } else {
                    "ies"
                }
            );
            for capability in &grant.capabilities {
                println!("  {capability}");
            }
            config.plugins.upsert_grant(grant);
            finish_plugin_change(&config, config_path)
        }
        PluginCommand::Remove(remove) => {
            if let Err(error) =
                anth_panel::plugins::bundle::remove(&plugins.root_path(), &remove.id)
            {
                eprintln!("[ERROR] {error}");
                return ExitCode::FAILURE;
            }
            match config.plugins.remove_grant(&remove.id) {
                Some(_) => println!("Removed {}", remove.id),
                None => {
                    println!("Removed the directory of {}, which had no grant", remove.id);
                }
            }
            finish_plugin_change(&config, config_path)
        }
        PluginCommand::Enable(toggle) => {
            let Some(grant) = config.plugins.grant(&toggle.id).cloned() else {
                eprintln!("[ERROR] plugin `{}` is not installed", toggle.id);
                return ExitCode::FAILURE;
            };
            let mut updated = grant;
            updated.enabled = !toggle.disable;
            let state = if updated.enabled {
                "enabled"
            } else {
                "disabled"
            };
            config.plugins.upsert_grant(updated);
            println!("{} is {state}", toggle.id);
            finish_plugin_change(&config, config_path)
        }
        PluginCommand::Approve(approve) => {
            if !approve.accept_capabilities {
                eprintln!(
                    "[ERROR] pass --accept-capabilities to confirm the capabilities of `{}`",
                    approve.id
                );
                return ExitCode::FAILURE;
            }
            let root = anth_panel::plugins::bundle::plugin_dir(&plugins.root_path(), &approve.id);
            let text = match std::fs::read_to_string(root.join("plugin.toml")) {
                Ok(text) => text,
                Err(error) => {
                    eprintln!("[ERROR] could not read the manifest: {error}");
                    return ExitCode::FAILURE;
                }
            };
            let manifest = match anth_panel::plugins::Manifest::parse(&text) {
                Ok(manifest) => manifest,
                Err(error) => {
                    eprintln!("[ERROR] {error}");
                    return ExitCode::FAILURE;
                }
            };
            let grant = anth_panel::plugins::PluginGrant::from_manifest(&manifest);
            println!(
                "Approved {} {} with {} capabilit{}",
                grant.id,
                grant.version,
                grant.capabilities.len(),
                if grant.capabilities.len() == 1 {
                    "y"
                } else {
                    "ies"
                }
            );
            for capability in &grant.capabilities {
                println!("  {capability}");
            }
            config.plugins.upsert_grant(grant);
            finish_plugin_change(&config, config_path)
        }
    }
}

fn finish_plugin_change(config: &Config, config_path: &str) -> ExitCode {
    match config.save(config_path) {
        Ok(()) => {
            println!("Restart the server to load the change.");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("[ERROR] {error}");
            ExitCode::FAILURE
        }
    }
}

/// Which services one detached session task serves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ServerRole {
    Daemon,
    Api,
    Run,
}

impl ServerRole {
    fn of(command: &Command) -> Option<Self> {
        match command {
            Command::Daemon => Some(Self::Daemon),
            Command::Api(_) => Some(Self::Api),
            Command::Run(_) => Some(Self::Run),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Daemon => "daemon",
            Self::Api => "api",
            Self::Run => "run",
        }
    }

    /// Whether a running task in `self` already binds what `requested` needs.
    ///
    /// `run` owns both the socket and the HTTP port, so it conflicts with a
    /// separate `daemon` or `api` in either direction. `daemon` and `api` do not
    /// conflict with each other: starting both is the point of the session.
    fn serves(self, requested: Self) -> bool {
        self == requested || self == Self::Run || requested == Self::Run
    }
}

struct BackgroundTask {
    role: ServerRole,
    handle: tokio::task::JoinHandle<()>,
}

/// Server subcommands the interactive session left running behind the prompt.
///
/// The prompt keeps reading lines while these serve, so one terminal can run the
/// daemon and the API and still send requests. A duplicate start is refused
/// rather than detached: `daemon::serve` unlinks the socket before binding, so a
/// second copy would steal the path from a live one and leave it unreachable.
#[derive(Default)]
struct BackgroundServers {
    tasks: Vec<BackgroundTask>,
}

impl BackgroundServers {
    /// Refuses a start that would collide with a server already detached.
    ///
    /// Kept separate from the spawning so no `RefCell` borrow is held across the
    /// readiness await: this call only inspects the registry.
    fn check(&mut self, command: &Command) -> Result<ServerRole, String> {
        let role = ServerRole::of(command).ok_or("not a server subcommand")?;

        // A task that already finished (a busy port, an unbindable socket) must
        // not block a retry.
        self.tasks.retain(|task| !task.handle.is_finished());
        if let Some(running) = self.tasks.iter().find(|task| task.role.serves(role)) {
            return Err(if running.role == role {
                format!(
                    "{} is already running in the background; quit the session to stop it",
                    role.label()
                )
            } else {
                format!(
                    "cannot start `{}`: `{}` already owns what it would bind; quit the session to stop it",
                    role.label(),
                    running.role.label()
                )
            });
        }
        Ok(role)
    }

    fn register(&mut self, role: ServerRole, handle: tokio::task::JoinHandle<()>) {
        self.tasks.push(BackgroundTask { role, handle });
    }

    /// Stops every detached server once the prompt closes.
    ///
    /// Aborting is enough: the session returns to `main` immediately, and
    /// dropping the runtime finishes the cancellation.
    fn shutdown(&mut self) {
        for task in self.tasks.drain(..) {
            task.handle.abort();
        }
    }
}

/// Spawns one serving subcommand and waits until it has bound its port.
///
/// The interactive session detaches a `daemon`, `api` or `run` and keeps its
/// prompt, so the next typed line is already waiting: without this handshake a
/// request could reach the socket before the listener exists.
async fn start_server(
    config_path: &str,
    command: &Command,
) -> Result<tokio::task::JoinHandle<()>, String> {
    let config = Config::load_or_create(config_path);
    let path = config_path.to_string();
    let (ready, bound) = tokio::sync::oneshot::channel();
    let handle = match command {
        Command::Daemon => tokio::spawn(async move {
            if let Err(error) =
                anth_panel::daemon::serve_with_ready(&config, Path::new(&path), Some(ready)).await
            {
                eprintln!("[ERROR] daemon stopped: {error}");
            }
        }),
        Command::Api(args) => {
            let args = args.clone();
            tokio::spawn(async move {
                if let Err(error) = serve_api(config, &path, &args, Some(ready)).await {
                    eprintln!("[ERROR] api stopped: {error}");
                }
            })
        }
        Command::Run(args) => {
            let args = args.clone();
            tokio::spawn(async move {
                if let Err(error) = serve_run(config, &path, &args, Some(ready)).await {
                    eprintln!("[ERROR] run stopped: {error}");
                }
            })
        }
        _ => unreachable!("only a server subcommand is detached"),
    };

    match bound.await {
        Ok(Ok(())) => Ok(handle),
        Ok(Err(_)) => {
            let _ = handle.await;
            Err("the service could not start; see the error above".to_string())
        }
        Err(_) => {
            let _ = handle.await;
            Err("the service stopped before it was listening".to_string())
        }
    }
}

/// Runs one command to completion and reports its exit code.
///
/// This is the single dispatcher: `main` calls it once, and the interactive
/// session calls it per line, so a typed command cannot drift from the
/// one-shot invocation. `background` is `Some` only in a session; there a
/// serving subcommand is detached instead of awaited, which is what lets the
/// prompt keep sending commands while the daemon and the API run.
async fn dispatch(
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
            &Config::load_or_create(config_path),
            std::path::Path::new(config_path),
        )
        .await
        .map(|()| ExitCode::SUCCESS)
        .map_err(|e| e.to_string()),
        Command::Api(args) => {
            serve_api(Config::load_or_create(config_path), config_path, args, None).await
        }
        Command::Run(args) => {
            serve_run(Config::load_or_create(config_path), config_path, args, None).await
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
async fn run_interactive(config_path: &str, first: Option<Command>) -> ExitCode {
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

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let config_path = cli.config_path.as_deref().unwrap_or(DEFAULT_CONFIG_PATH);

    if cli.interactive {
        return run_interactive(config_path, cli.command).await;
    }

    match &cli.command {
        Some(command) => dispatch(config_path, command, None).await,
        // A bare invocation has nothing to run; clap cannot require the
        // subcommand because `-i` alone is valid.
        None => {
            eprintln!("[ERROR] a command is required; run `anth-panel --help`");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
