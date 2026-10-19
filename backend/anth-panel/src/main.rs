use std::io::{Read, Write};
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use anth_panel::cli::{
    Cli, Command, EndpointArgs, FileArgs, FileCommand, FileDeleteArgs, FileReadArgs,
    FileUploadArgs, NetworkArgs, NetworkCommand, NetworkStateArgs, ProcessArgs, ProcessCommand,
    ProcessSignalArgs, WatchArgs,
};
use anth_panel::config::{Config, DEFAULT_CONFIG_PATH};
use anth_panel::stream;
use anth_panel::{AppState, request_for, router};
use anth_proto::{AnthClient, DEFAULT_FILE_CHUNK, FileChunk, Request, Response};
use tokio::net::TcpListener;

const DEFAULT_ADDR: &str = "0.0.0.0:8080";

fn resolve_addr(args: &EndpointArgs) -> String {
    args.addr
        .clone()
        .or_else(|| std::env::var("ANTH_PANEL_ADDR").ok())
        .unwrap_or_else(|| DEFAULT_ADDR.to_string())
}

async fn serve_api(config: Config, args: &EndpointArgs) -> Result<ExitCode, String> {
    let address = resolve_addr(args);
    let listener = TcpListener::bind(&address)
        .await
        .map_err(|e| format!("Failed to bind to address {address}: {e}"))?;

    println!("Server is listening on {address}");
    axum::serve(listener, router(AppState::new(config)))
        .await
        .map_err(|e| format!("Server error: {e}"))?;
    Ok(ExitCode::SUCCESS)
}

/// Keeps the daemon task alive for as long as the API runs: binding the HTTP
/// port first surfaces address-in-use errors before the socket is touched.
async fn serve_run(config: Config, args: &EndpointArgs) -> Result<ExitCode, String> {
    let address = resolve_addr(args);
    let listener = TcpListener::bind(&address)
        .await
        .map_err(|e| format!("Failed to bind to address {address}: {e}"))?;

    let daemon_config = config.clone();
    let daemon = tokio::spawn(async move {
        if let Err(e) = anth_panel::daemon::serve(&daemon_config).await {
            eprintln!("Daemon stopped: {e}");
        }
    });

    println!("Daemon and server are running (http://{address})");
    let result = axum::serve(listener, router(AppState::new(config)))
        .await
        .map_err(|e| format!("Server error: {e}"));

    daemon.abort();
    let _ = daemon.await;
    result.map(|_| ExitCode::SUCCESS)
}

/// How a response becomes one printed frame.
fn render_frame(response: &Response) -> String {
    match response {
        Response::HostInfo(info) => anth_panel::render_host_info(info),
        Response::ProcessSnapshot(snapshot) => anth_panel::render_process_snapshot(snapshot),
        Response::ProcessSignalled { pid, signal } => {
            format!("[OK] sent {signal} to process {pid}")
        }
        Response::NetworkSnapshot(snapshot) => anth_panel::render_network_snapshot(snapshot),
        Response::NetworkStateChanged { name, up } => {
            let state = if *up { "up" } else { "down" };
            format!("[OK] brought {name} {state}")
        }
        Response::NetworkError(error) => format!("[ERROR] {}", error.detail),
        Response::FileListing(listing) => anth_panel::render_file_listing(listing),
        Response::FileChunk(chunk) => format!(
            "[ERROR] unexpected file chunk for {} ({} bytes)",
            chunk.path,
            chunk.bytes.len()
        ),
        Response::FileWritten(written) => {
            format!("[OK] wrote {} ({} bytes)", written.path, written.size)
        }
        Response::FileCreated { path } => format!("[OK] created {path}"),
        Response::FileDeleted { path } => format!("[OK] deleted {path}"),
        Response::FileRenamed { from, to } => format!("[OK] renamed {from} to {to}"),
        Response::FileWriteStarted { id } => format!("[OK] upload {id} started"),
        Response::FileWriteAccepted { id, received } => {
            format!("[OK] upload {id} received {received} bytes")
        }
        Response::FileWriteAborted { id } => format!("[OK] upload {id} aborted"),
        Response::FileError(error) => format!("[ERROR] {}", error.detail),
        Response::Ok { message } => format!("[OK] {message}"),
        Response::Error { detail } => format!("[ERROR] {detail}"),
        // A CLI subcommand never opens a terminal, so these are only reachable
        // if a daemon sends frames nobody asked for.
        Response::TerminalOpened(opened) => {
            format!("[ERROR] unexpected terminal session {}", opened.id)
        }
        Response::TerminalOutput { id, bytes } => {
            format!(
                "[ERROR] unexpected terminal output for session {id} ({} bytes)",
                bytes.len()
            )
        }
        Response::TerminalExited { id, status } => {
            format!("[ERROR] unexpected terminal exit for session {id} (status {status})")
        }
    }
}

/// How often a subcommand repeats, or `None` to read once. A subcommand opts
/// into streaming by answering `Some` here; nothing else needs to change.
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

fn confirm_signal(args: &ProcessSignalArgs) -> Result<bool, String> {
    use std::io::{IsTerminal, Write};

    if args.yes {
        return Ok(true);
    }

    let stdin = std::io::stdin();
    if !stdin.is_terminal() {
        return Err("refusing to signal without --yes when stdin is not a terminal".to_string());
    }

    print!("Send {} to process {}? [y/N] ", args.signal, args.pid);
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

fn confirm_network_state(args: &NetworkStateArgs, up: bool) -> Result<bool, String> {
    use std::io::{IsTerminal, Write};

    if args.yes {
        return Ok(true);
    }

    let stdin = std::io::stdin();
    if !stdin.is_terminal() {
        return Err(
            "refusing to change a network interface without --yes when stdin is not a terminal"
                .to_string(),
        );
    }

    let action = if up { "Bring up" } else { "Bring down" };
    print!(
        "{action} interface `{}`? This can disconnect the host. [y/N] ",
        args.name
    );
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

fn confirm_delete(args: &FileDeleteArgs) -> Result<bool, String> {
    use std::io::{IsTerminal, Write};

    if args.yes {
        return Ok(true);
    }

    let stdin = std::io::stdin();
    if !stdin.is_terminal() {
        return Err("refusing to delete without --yes when stdin is not a terminal".to_string());
    }

    let verb = if args.recursive {
        "Recursively delete"
    } else {
        "Delete"
    };
    print!("{verb} `{}`? [y/N] ", args.path);
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
            render_frame,
        )
        .await,
    )
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let config_path = cli.config_path.as_deref().unwrap_or(DEFAULT_CONFIG_PATH);

    if let Some(code) = send_command(config_path, &cli.command).await {
        return code;
    }

    let result = match &cli.command {
        Command::Daemon => anth_panel::daemon::serve(&Config::load_or_create(config_path))
            .await
            .map(|()| ExitCode::SUCCESS)
            .map_err(|e| e.to_string()),
        Command::Api(args) => serve_api(Config::load_or_create(config_path), args).await,
        Command::Run(args) => serve_run(Config::load_or_create(config_path), args).await,
        Command::Terminal(args) => {
            let config = Config::load_or_default(config_path);
            Ok(anth_panel::terminal_client::run(Path::new(&config.socket_path), args).await)
        }
        Command::Echo { .. }
        | Command::HostInfo(_)
        | Command::Process(_)
        | Command::Network(_)
        | Command::File(_) => {
            unreachable!("handled by send_command")
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
