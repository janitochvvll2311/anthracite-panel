use anth_panel::cli::{
    Cli, Command, FileArgs, FileCommand, FileDeleteArgs, FileListArgs, FileMkdirArgs, FileReadArgs,
    FileRenameArgs, FileUploadArgs, NetworkArgs, NetworkCommand, NetworkStateArgs, ProcessArgs,
    ProcessCommand, ProcessSignalArgs, WatchArgs,
};
use anth_panel::{handle_response, request_for};
use anth_proto::{ProcessSignal, Request, Response};
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
fn request_for_network_builds_list_and_state_requests() {
    match request_for(&Command::Network(NetworkArgs {
        command: NetworkCommand::List(WatchArgs::default()),
    })) {
        Some(Request::NetworkList) => {}
        other => panic!("expected a NetworkList request, got {other:?}"),
    }

    match request_for(&Command::Network(NetworkArgs {
        command: NetworkCommand::Up(NetworkStateArgs {
            name: "eth0".to_string(),
            yes: true,
        }),
    })) {
        Some(Request::NetworkSetState { name, up }) => {
            assert_eq!(name, "eth0");
            assert!(up);
        }
        other => panic!("expected a NetworkSetState request, got {other:?}"),
    }

    match request_for(&Command::Network(NetworkArgs {
        command: NetworkCommand::Down(NetworkStateArgs {
            name: "eth0".to_string(),
            yes: false,
        }),
    })) {
        Some(Request::NetworkSetState { name, up }) => {
            assert_eq!(name, "eth0");
            assert!(!up);
        }
        other => panic!("expected a NetworkSetState request, got {other:?}"),
    }
}

#[test]
fn cli_parses_network_list_and_state_subcommands() {
    let cli = Cli::try_parse_from(["anth-panel", "network", "list", "--watch", "1s"]).unwrap();
    match cli.command {
        Command::Network(NetworkArgs {
            command: NetworkCommand::List(args),
        }) => assert_eq!(args.watch.as_deref(), Some("1s")),
        other => panic!("expected network list, got {other:?}"),
    }

    let cli = Cli::try_parse_from(["anth-panel", "network", "down", "eth0", "--yes"]).unwrap();
    match cli.command {
        Command::Network(NetworkArgs {
            command: NetworkCommand::Down(args),
        }) => {
            assert_eq!(args.name, "eth0");
            assert!(args.yes);
        }
        other => panic!("expected network down, got {other:?}"),
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

#[test]
fn request_for_process_list_and_signal() {
    let list = Command::Process(ProcessArgs {
        command: ProcessCommand::List(WatchArgs::default()),
    });
    assert!(matches!(request_for(&list), Some(Request::ProcessList)));

    let signal = Command::Process(ProcessArgs {
        command: ProcessCommand::Signal(ProcessSignalArgs {
            pid: 42,
            signal: ProcessSignal::Term,
            yes: true,
        }),
    });
    match request_for(&signal) {
        Some(Request::ProcessSignal { pid, signal }) => {
            assert_eq!(pid, 42);
            assert_eq!(signal, ProcessSignal::Term);
        }
        other => panic!("expected ProcessSignal, got {other:?}"),
    }
}

#[test]
fn cli_parses_process_list_and_its_ls_alias() {
    for name in ["list", "ls"] {
        let cli = Cli::try_parse_from(["anth-panel", "process", name]).unwrap();
        match cli.command {
            Command::Process(ProcessArgs {
                command: ProcessCommand::List(args),
            }) => assert_eq!(args.watch, None),
            other => panic!("expected process list, got {other:?}"),
        }
    }
}

#[test]
fn cli_process_list_accepts_watch() {
    let cli = Cli::try_parse_from(["anth-panel", "process", "list", "--watch", "500ms"]).unwrap();
    match cli.command {
        Command::Process(ProcessArgs {
            command: ProcessCommand::List(args),
        }) => assert_eq!(args.watch.as_deref(), Some("500ms")),
        other => panic!("expected process list, got {other:?}"),
    }
}

#[test]
fn cli_parses_process_signal() {
    let cli =
        Cli::try_parse_from(["anth-panel", "process", "signal", "42", "TERM", "--yes"]).unwrap();

    match cli.command {
        Command::Process(ProcessArgs {
            command: ProcessCommand::Signal(args),
        }) => {
            assert_eq!(args.pid, 42);
            assert_eq!(args.signal, ProcessSignal::Term);
            assert!(args.yes);
        }
        other => panic!("expected process signal, got {other:?}"),
    }
}

#[test]
fn cli_rejects_an_unknown_process_signal() {
    assert!(Cli::try_parse_from(["anth-panel", "process", "signal", "42", "nope"]).is_err());
}

#[test]
fn request_for_file_list_mkdir_rename_and_delete() {
    let cases = [
        (
            Command::File(FileArgs {
                command: FileCommand::List(FileListArgs {
                    path: "/tmp".to_string(),
                }),
            }),
            Request::FileList {
                path: "/tmp".to_string(),
            },
        ),
        (
            Command::File(FileArgs {
                command: FileCommand::Mkdir(FileMkdirArgs {
                    path: "/tmp/new".to_string(),
                    parents: true,
                }),
            }),
            Request::FileMkdir {
                path: "/tmp/new".to_string(),
                recursive: true,
            },
        ),
        (
            Command::File(FileArgs {
                command: FileCommand::Rename(FileRenameArgs {
                    from: "/tmp/a".to_string(),
                    to: "/tmp/b".to_string(),
                }),
            }),
            Request::FileRename {
                from: "/tmp/a".to_string(),
                to: "/tmp/b".to_string(),
            },
        ),
        (
            Command::File(FileArgs {
                command: FileCommand::Delete(FileDeleteArgs {
                    path: "/tmp/a".to_string(),
                    recursive: true,
                    yes: true,
                }),
            }),
            Request::FileDelete {
                path: "/tmp/a".to_string(),
                recursive: true,
            },
        ),
    ];

    for (command, expected) in cases {
        let request = request_for(&command).expect("expected a file request");
        assert_eq!(request.to_json().unwrap(), expected.to_json().unwrap());
    }
}

#[test]
fn read_and_upload_are_handled_outside_the_generic_request_path() {
    assert!(
        request_for(&Command::File(FileArgs {
            command: FileCommand::Read(FileReadArgs {
                path: "/tmp/a".to_string(),
                output: None,
            }),
        }))
        .is_none()
    );
    assert!(
        request_for(&Command::File(FileArgs {
            command: FileCommand::Upload(FileUploadArgs {
                local: "a".to_string(),
                dest: "/tmp/a".to_string(),
                overwrite: false,
            }),
        }))
        .is_none()
    );
}

#[test]
fn file_command_has_no_aliases() {
    assert!(Cli::try_parse_from(["anth-panel", "file", "list", "/"]).is_ok());
    assert!(Cli::try_parse_from(["anth-panel", "fs", "list", "/"]).is_err());
    assert!(Cli::try_parse_from(["anth-panel", "file", "ls", "/"]).is_err());
    assert!(Cli::try_parse_from(["anth-panel", "file", "cat", "/a"]).is_err());
    assert!(Cli::try_parse_from(["anth-panel", "file", "rm", "/a"]).is_err());
}

#[test]
fn file_commands_parse_their_flags() {
    let cli = Cli::try_parse_from([
        "anth-panel",
        "file",
        "upload",
        "local.bin",
        "/remote.bin",
        "--overwrite",
    ])
    .unwrap();
    match cli.command {
        Command::File(FileArgs {
            command: FileCommand::Upload(args),
        }) => {
            assert_eq!(args.local, "local.bin");
            assert_eq!(args.dest, "/remote.bin");
            assert!(args.overwrite);
        }
        other => panic!("expected file upload, got {other:?}"),
    }

    let cli = Cli::try_parse_from([
        "anth-panel",
        "file",
        "delete",
        "/tmp/a",
        "--recursive",
        "--yes",
    ])
    .unwrap();
    match cli.command {
        Command::File(FileArgs {
            command: FileCommand::Delete(args),
        }) => {
            assert_eq!(args.path, "/tmp/a");
            assert!(args.recursive);
            assert!(args.yes);
        }
        other => panic!("expected file delete, got {other:?}"),
    }
}
