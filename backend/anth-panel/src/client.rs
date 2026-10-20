use anth_proto::{Request, Response};

use crate::cli::{
    AccountCommand, AccountGroupCommand, AccountUserCommand, Command, FileCommand, FirewallCommand,
    NetworkCommand, ProcessCommand, VolumeCommand,
};
use crate::render;

/// Maps a client-only subcommand to the request it sends over the socket.
/// Returns `None` for subcommands that run locally (`daemon`, `api`, `run`);
/// those never travel as requests.
pub fn request_for(command: &Command) -> Option<Request> {
    match command {
        Command::Echo { message } => Some(Request::Echo {
            message: message.clone(),
        }),
        Command::HostInfo(_) => Some(Request::HostInfo),
        Command::Process(args) => match &args.command {
            ProcessCommand::List(_) => Some(Request::ProcessList),
            ProcessCommand::Signal(signal) => Some(Request::ProcessSignal {
                pid: signal.pid,
                signal: signal.signal,
            }),
        },
        Command::Network(args) => match &args.command {
            NetworkCommand::List(_) => Some(Request::NetworkList),
            NetworkCommand::Up(args) => Some(Request::NetworkSetState {
                name: args.name.clone(),
                up: true,
            }),
            NetworkCommand::Down(args) => Some(Request::NetworkSetState {
                name: args.name.clone(),
                up: false,
            }),
        },
        Command::Firewall(args) => match &args.command {
            FirewallCommand::Status => Some(Request::FirewallStatus),
            FirewallCommand::Allow(rule) => Some(Request::FirewallAddRule {
                action: anth_proto::FirewallAction::Allow,
                port: rule.spec.port,
                protocol: rule.spec.protocol,
                comment: rule.comment.clone(),
            }),
            FirewallCommand::Deny(rule) => Some(Request::FirewallAddRule {
                action: anth_proto::FirewallAction::Deny,
                port: rule.spec.port,
                protocol: rule.spec.protocol,
                comment: rule.comment.clone(),
            }),
            FirewallCommand::Delete(args) => Some(Request::FirewallDeleteRule {
                handle: args.handle,
            }),
            FirewallCommand::Flush(_) => Some(Request::FirewallFlush),
        },
        Command::Volume(args) => match &args.command {
            VolumeCommand::List => Some(Request::VolumeList),
            VolumeCommand::Mount(args) => Some(Request::VolumeMount {
                device: args.device.clone(),
                target: args.target.clone(),
                fs_type: args.fs_type.clone(),
                read_only: args.read_only,
                allow_removable: args.allow_removable,
            }),
            VolumeCommand::Unmount(args) => Some(Request::VolumeUnmount {
                target: args.target.clone(),
            }),
        },
        Command::Account(args) => match &args.command {
            AccountCommand::List => Some(Request::AccountList),
            AccountCommand::User(args) => match &args.command {
                AccountUserCommand::Create(args) => Some(Request::AccountCreate {
                    name: args.name.clone(),
                    uid: args.uid,
                    gid: args.gid,
                    gecos: args.gecos.clone(),
                    home: args.home.clone(),
                    shell: args.shell.clone(),
                    system: args.system,
                }),
                AccountUserCommand::Modify(args) => {
                    let locked = match (args.lock, args.unlock) {
                        (true, _) => Some(true),
                        (_, true) => Some(false),
                        _ => None,
                    };
                    Some(Request::AccountModify {
                        name: args.name.clone(),
                        gecos: args.gecos.clone(),
                        shell: args.shell.clone(),
                        home: args.home.clone(),
                        primary_gid: args.gid,
                        locked,
                    })
                }
                AccountUserCommand::Delete(args) => Some(Request::AccountDelete {
                    name: args.name.clone(),
                    remove_home: args.remove_home,
                }),
                // The password is read before this point and handed over by
                // `send_account_password`, never assembled here.
                AccountUserCommand::Password(_) => None,
            },
            AccountCommand::Group(args) => match &args.command {
                AccountGroupCommand::Create(args) => Some(Request::AccountGroupCreate {
                    name: args.name.clone(),
                    gid: args.gid,
                    system: args.system,
                }),
                AccountGroupCommand::Modify(args) => Some(Request::AccountGroupModify {
                    name: args.name.clone(),
                    gid: args.gid,
                    members: args.members.clone(),
                }),
                AccountGroupCommand::Delete(args) => Some(Request::AccountGroupDelete {
                    name: args.name.clone(),
                }),
            },
        },
        Command::File(args) => match &args.command {
            FileCommand::List(list) => Some(Request::FileList {
                path: list.path.clone(),
            }),
            FileCommand::Read(_) | FileCommand::Upload(_) => None,
            FileCommand::Mkdir(args) => Some(Request::FileMkdir {
                path: args.path.clone(),
                recursive: args.parents,
            }),
            FileCommand::Rename(args) => Some(Request::FileRename {
                from: args.from.clone(),
                to: args.to.clone(),
            }),
            FileCommand::Delete(args) => Some(Request::FileDelete {
                path: args.path.clone(),
                recursive: args.recursive,
            }),
        },
        Command::Terminal(_)
        | Command::Auth(_)
        | Command::Config(_)
        | Command::Daemon
        | Command::Api(_)
        | Command::Run(_) => None,
    }
}

pub fn handle_response(response: &Response) {
    let frame = render::render_response(response);
    match response {
        Response::Error { .. }
        | Response::FileError(_)
        | Response::NetworkError(_)
        | Response::VolumeError(_)
        | Response::FirewallError(_) => {
            eprintln!("{frame}");
        }
        _ => println!("{frame}"),
    }
}
