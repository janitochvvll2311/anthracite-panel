use super::super::main_prelude::*;
use super::confirm::*;
use super::dispatch::watch_interval;

/// Sends `account user password`. The password is read here, never assembled
/// from argv, and travels only in the request body.
pub async fn send_account_password(config_path: &str, args: &AccountUserPasswordArgs) -> ExitCode {
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

pub fn client_config(config_path: &str) -> Config {
    Config::load_or_default(config_path)
}

pub async fn connect_client(config: &Config) -> Result<AnthClient, ExitCode> {
    match tokio::net::UnixStream::connect(Path::new(&config.socket_path)).await {
        Ok(stream) => Ok(AnthClient::new(stream)),
        Err(e) => {
            eprintln!("Failed to connect to socket: {e}");
            Err(ExitCode::FAILURE)
        }
    }
}

pub async fn exchange(client: &mut AnthClient, request: &Request) -> Result<Response, String> {
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

pub async fn send_file_read(config_path: &str, args: &FileReadArgs) -> ExitCode {
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

pub async fn send_file_upload(config_path: &str, args: &FileUploadArgs) -> ExitCode {
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
pub async fn send_command(config_path: &str, command: &Command) -> Option<ExitCode> {
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
pub async fn write_text_file(config_path: &str, args: &FileWriteArgs) -> ExitCode {
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
