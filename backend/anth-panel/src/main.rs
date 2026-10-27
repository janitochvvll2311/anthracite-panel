//! `anth-panel` binary: daemon, HTTP API and CLI client.

use std::io::{IsTerminal, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use anth_panel::api::{self, AppState};
use anth_panel::auth::AuthService;
use anth_panel::client::Client;
use anth_panel::config::Config;
use anth_panel::dispatch::Daemon;
use anth_panel::lifecycle::{self, Lifecycle};
use anth_panel::log::{Format, Level, Logger, Out, Settings};
use anth_panel::{render, socket};
use anth_wire::{Request, Response, parse_interval};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use clap::{Parser, Subcommand};
use tokio::net::TcpListener;

type CliResult = Result<(), (u8, String)>;

#[derive(Parser, Debug)]
#[command(name = "anth-panel", version, about = "Anthracite Panel")]
struct Cli {
    /// Path to the configuration file.
    #[arg(long, global = true, value_name = "FILE")]
    config: Option<PathBuf>,

    /// Log level: error, warn, info, debug or trace.
    #[arg(long, global = true)]
    log_level: Option<String>,

    /// Log format: human, json or off.
    #[arg(long, global = true)]
    log_format: Option<String>,

    /// Log destination: stdout or stderr.
    #[arg(long, global = true)]
    log_out: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Serve the Unix socket and the HTTP API in one process.
    Run {
        /// HTTP bind address.
        #[arg(long, default_value = "0.0.0.0:8080")]
        addr: String,
        /// Let `reboot` take the host down.
        #[arg(long)]
        allow_reboot: bool,
    },
    /// Serve the Unix socket only.
    Daemon {
        /// Let `reboot` take the host down.
        #[arg(long)]
        allow_reboot: bool,
    },
    /// Replace the running daemon with a fresh copy of itself, re-reading the
    /// configuration.
    Restart,
    /// Ask the host to reboot.
    Reboot {
        /// Seconds to wait before the host goes down (at most 300).
        #[arg(long, default_value_t = 0)]
        delay: u32,
    },
    /// Browse and read files under the configured root.
    Files {
        #[command(subcommand)]
        command: FilesCommand,
    },
    /// Print a snapshot of the host the daemon runs on.
    HostInfo {
        /// Keep reading every INTERVAL instead of printing once.
        #[arg(
            long,
            value_name = "INTERVAL",
            num_args = 0..=1,
            default_missing_value = anth_wire::DEFAULT_INTERVAL
        )]
        watch: Option<String>,
    },
    /// Read, validate or change the configuration.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
}

#[derive(Subcommand, Debug)]
enum ConfigCommand {
    /// Print the configuration file.
    Get,
    /// Parse and validate the configuration without writing.
    Validate,
    /// Change one dotted key, preserving comments and key order.
    Set {
        /// Dotted key, e.g. files.write or auth.username.
        key: String,
        /// New value, parsed as a TOML literal.
        value: String,
    },
}

#[derive(Subcommand, Debug)]
enum FilesCommand {
    /// List a directory under the file root.
    List {
        /// Path relative to the file root.
        #[arg(long, default_value = "/", value_name = "PATH")]
        path: String,
        /// Maximum entries in this page (1-1000).
        #[arg(long)]
        limit: Option<u16>,
        /// Opaque cursor from a previous page.
        #[arg(long)]
        cursor: Option<String>,
    },
    /// Print a UTF-8 text file.
    Read {
        /// Path relative to the file root.
        #[arg(long, value_name = "PATH")]
        path: String,
        /// Byte offset to start at.
        #[arg(long, default_value_t = 0)]
        offset: u64,
        /// Maximum bytes to read.
        #[arg(long)]
        max_bytes: Option<u64>,
    },
    /// Delete a file under the file root. Requires files.write.
    Delete {
        /// Path relative to the file root.
        #[arg(long, value_name = "PATH")]
        path: String,
    },
    /// Write a UTF-8 file under the file root; the content comes from stdin.
    Write {
        /// Path relative to the file root.
        #[arg(long, value_name = "PATH")]
        path: String,
        /// Fail instead of replacing an existing file.
        #[arg(long)]
        create: bool,
    },
    /// Upload arbitrary bytes from stdin. Requires files.write.
    Upload {
        /// Path relative to the file root.
        #[arg(long, value_name = "PATH")]
        path: String,
        /// Fail instead of replacing an existing file.
        #[arg(long)]
        create: bool,
    },
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err((code, message)) => {
            eprintln!("{message}");
            ExitCode::from(code)
        }
    }
}

async fn run() -> CliResult {
    let cli = Cli::parse();
    let path = cli
        .config
        .clone()
        .unwrap_or_else(Config::default_config_path);

    match &cli.command {
        Command::Config { command } => config_command(&path, command, &cli),
        Command::HostInfo { watch } => host_info(&path, watch.clone()).await,
        Command::Daemon { allow_reboot } => serve(&path, None, *allow_reboot, &cli).await,
        Command::Restart => lifecycle_command(&path, LifecycleRequest::Restart).await,
        Command::Reboot { delay } => {
            lifecycle_command(&path, LifecycleRequest::Reboot(*delay)).await
        }
        Command::Files { command } => files_command(&path, command).await,
        Command::Run { addr, allow_reboot } => {
            serve(&path, Some(addr.clone()), *allow_reboot, &cli).await
        }
    }
}

fn config_command(path: &std::path::Path, command: &ConfigCommand, cli: &Cli) -> CliResult {
    let (settings, problems) = client_settings(None, cli);
    for problem in problems {
        eprintln!("{problem}");
    }
    let _logger = Logger::with_out(settings);
    match command {
        ConfigCommand::Get => {
            let text = std::fs::read_to_string(path)
                .map_err(|error| (1, format!("reading {}: {error}", path.display())))?;
            print!("{text}");
            Ok(())
        }
        ConfigCommand::Validate => {
            Config::load(path).map_err(|error| (1, error))?;
            println!("{} is valid", path.display());
            Ok(())
        }
        ConfigCommand::Set { key, value } => {
            Config::set_key(path, key, value).map_err(|error| (2, error))?;
            println!("{} updated", path.display());
            Ok(())
        }
    }
}

async fn serve(
    path: &std::path::Path,
    addr: Option<String>,
    allow_reboot: bool,
    cli: &Cli,
) -> CliResult {
    let config = Config::load_or_create(path).map_err(|error| (1, error))?;
    let paths = config.paths(path).map_err(|error| (1, error))?;
    prepare_state_dir(&paths.state_dir)?;

    let (settings, problems) = client_settings(Some(&config), cli);
    for problem in problems {
        eprintln!("{problem}");
    }
    let logger = Arc::new(Logger::with_out(settings));
    let (auth, generated) = AuthService::load(&config.auth, &paths)
        .map_err(|error| (1, format!("authentication: {error}")))?;
    if let Some(password) = generated {
        eprintln!(
            "\nPanel authentication is now enabled.\nUsername: {}\nPassword: {password}\nThis password is shown once and stored only as a hash.\n",
            config.auth.username
        );
    } else if auth.state().bootstrap == anth_panel::auth::Bootstrap::Web {
        eprintln!(
            "Panel registration is open. POST /api/auth/register or use the web UI to set the first password."
        );
    }

    let daemon = Arc::new(Daemon::with_lifecycle(
        config.clone(),
        paths.clone(),
        Arc::clone(&logger),
        Arc::new(Lifecycle::new(Arc::new(lifecycle::System), allow_reboot)),
    ));
    let socket_listener =
        socket::bind(&paths.socket).map_err(|error| (1, format!("socket: {error}")))?;

    if let Some(addr) = addr {
        let http_listener = TcpListener::bind(&addr)
            .await
            .map_err(|error| (1, format!("binding HTTP {addr}: {error}")))?;
        let state = AppState::new(Arc::clone(&daemon), auth, logger);
        let router = api::router(state);
        eprintln!(
            "anth-panel {} listening on http://{addr} and {}",
            env!("CARGO_PKG_VERSION"),
            paths.socket.display()
        );
        let http = axum::serve(
            http_listener,
            router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        );
        let socket = socket::serve(socket_listener, Arc::clone(&daemon));
        tokio::select! {
            result = http => result.map_err(|error| (1, format!("HTTP server: {error}"))),
            result = socket => result.map_err(|error| (1, format!("socket server: {error}"))),
        }
    } else {
        eprintln!(
            "anth-panel {} listening on {}",
            env!("CARGO_PKG_VERSION"),
            paths.socket.display()
        );
        socket::serve(socket_listener, daemon)
            .await
            .map_err(|error| (1, format!("socket server: {error}")))
    }
}

enum LifecycleRequest {
    Restart,
    Reboot(u32),
}

/// Opens the socket and performs the handshake; every client command starts here.
async fn connect(path: &std::path::Path) -> Result<Client, (u8, String)> {
    let config = Config::load_or_default(path);
    let paths = config.paths(path).map_err(|error| (1, error))?;
    let mut client = Client::connect(&paths.socket)
        .await
        .map_err(|error| (1, error.to_string()))?;
    client
        .hello()
        .await
        .map_err(|error| (1, error.to_string()))?;
    Ok(client)
}

async fn lifecycle_command(path: &std::path::Path, command: LifecycleRequest) -> CliResult {
    let mut client = connect(path).await?;

    let request = match command {
        LifecycleRequest::Restart => Request::Restart,
        LifecycleRequest::Reboot(delay) => Request::SystemReboot { delay_s: delay },
    };
    let response = client
        .call(&request)
        .await
        .map_err(|error| (error.kind.exit_code(), error.to_string()))?;
    match response {
        Response::Restarting { pid } => {
            println!("restarting the panel (pid {pid}); it re-reads the configuration");
            Ok(())
        }
        Response::Rebooting { delay_s } => {
            if delay_s == 0 {
                println!("the host is rebooting");
            } else {
                println!("the host reboots in {delay_s}s");
            }
            Ok(())
        }
        other => Err((1, format!("unexpected response: {other:?}"))),
    }
}

async fn files_command(path: &std::path::Path, command: &FilesCommand) -> CliResult {
    let mut client = connect(path).await?;

    match command {
        FilesCommand::List {
            path,
            limit,
            cursor,
        } => {
            let response = client
                .call(&Request::FilesList {
                    path: path.clone(),
                    limit: *limit,
                    cursor: cursor.clone(),
                })
                .await
                .map_err(|error| (error.kind.exit_code(), error.to_string()))?;
            match response {
                Response::FilesList(page) => {
                    print!("{}", render::file_list(&page));
                    println!();
                    Ok(())
                }
                other => Err((1, format!("unexpected response: {other:?}"))),
            }
        }
        FilesCommand::Read {
            path,
            offset,
            max_bytes,
        } => {
            let response = client
                .call(&Request::FilesRead {
                    path: path.clone(),
                    offset: *offset,
                    max_bytes: *max_bytes,
                })
                .await
                .map_err(|error| (error.kind.exit_code(), error.to_string()))?;
            match response {
                Response::FilesRead(chunk) => {
                    print!("{}", chunk.text);
                    if !chunk.text.ends_with('\n') {
                        println!();
                    }
                    if !chunk.eof {
                        eprintln!(
                            "[more at offset {}]",
                            chunk.offset + u64::try_from(chunk.text.len()).unwrap_or(u64::MAX)
                        );
                    }
                    Ok(())
                }
                other => Err((1, format!("unexpected response: {other:?}"))),
            }
        }
        FilesCommand::Delete { path } => {
            let response = client
                .call(&Request::FilesDelete { path: path.clone() })
                .await
                .map_err(|error| (error.kind.exit_code(), error.to_string()))?;
            match response {
                Response::FilesDeleted { path } => {
                    println!("deleted {path}");
                    Ok(())
                }
                other => Err((1, format!("unexpected response: {other:?}"))),
            }
        }
        FilesCommand::Write { path, create } => {
            let mut content = String::new();
            std::io::stdin()
                .read_to_string(&mut content)
                .map_err(|error| (1, format!("reading stdin: {error}")))?;
            let response = client
                .call(&Request::FilesWrite {
                    path: path.clone(),
                    content,
                    create: *create,
                })
                .await
                .map_err(|error| (error.kind.exit_code(), error.to_string()))?;
            match response {
                Response::FilesWritten(written) => {
                    println!("wrote {} ({} bytes)", written.path, written.size);
                    Ok(())
                }
                other => Err((1, format!("unexpected response: {other:?}"))),
            }
        }
        FilesCommand::Upload { path, create } => {
            let mut content = Vec::new();
            std::io::stdin()
                .read_to_end(&mut content)
                .map_err(|error| (1, format!("reading stdin: {error}")))?;
            let response = client
                .call(&Request::FilesUpload {
                    path: path.clone(),
                    content_base64: STANDARD.encode(&content),
                    create: *create,
                })
                .await
                .map_err(|error| (error.kind.exit_code(), error.to_string()))?;
            match response {
                Response::FilesWritten(written) => {
                    println!("wrote {} ({} bytes)", written.path, written.size);
                    Ok(())
                }
                other => Err((1, format!("unexpected response: {other:?}"))),
            }
        }
    }
}

async fn host_info(path: &std::path::Path, watch: Option<String>) -> CliResult {
    let mut client = connect(path).await?;

    let Some(interval) = watch else {
        let response = client
            .call(&Request::HostInfo)
            .await
            .map_err(|error| (1, error.to_string()))?;
        print_host_info(&response)?;
        return Ok(());
    };

    let interval = parse_interval(&interval).map_err(|error| (2, error))?;
    let terminal = std::io::stdout().is_terminal();
    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut first = true;
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => return Ok(()),
            _ = ticker.tick() => {
                let response = client
                    .call(&Request::HostInfo)
                    .await
                    .map_err(|error| (1, error.to_string()))?;
                let text = render_response(&response)?;
                if terminal {
                    println!("\x1b[2J\x1b[H{text}");
                } else {
                    if !first {
                        println!();
                    }
                    println!("{text}");
                }
                std::io::stdout().flush().ok();
                first = false;
            }
        }
    }
}

fn print_host_info(response: &Response) -> CliResult {
    println!("{}", render_response(response)?);
    Ok(())
}

fn render_response(response: &Response) -> Result<String, (u8, String)> {
    match response {
        Response::HostInfo(info) => Ok(render::host_info(info)),
        Response::Error(error) => Err((error.kind.exit_code(), error.message.clone())),
        other => Err((1, format!("unexpected response: {other:?}"))),
    }
}

fn client_settings(config: Option<&Config>, cli: &Cli) -> (Settings, Vec<String>) {
    let default = config
        .map(|config| Settings {
            level: config.log.level,
            format: config.log.format,
            out: config.log.out,
        })
        .unwrap_or(Settings {
            level: Level::Info,
            format: Format::Human,
            out: Out::Stdout,
        });
    Settings::resolve_with(
        default,
        cli.log_format.as_deref(),
        cli.log_level.as_deref(),
        cli.log_out.as_deref(),
        &|name| std::env::var(name).ok(),
    )
}

fn prepare_state_dir(path: &std::path::Path) -> CliResult {
    use std::os::unix::fs::PermissionsExt;

    std::fs::create_dir_all(path)
        .map_err(|error| (1, format!("creating {}: {error}", path.display())))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
        .map_err(|error| (1, format!("setting mode on {}: {error}", path.display())))?;
    Ok(())
}
