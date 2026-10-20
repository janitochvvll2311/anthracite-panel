//! Request dispatch as a chain of handlers.
//!
//! Every request walks the chain from the first handler to the last one, and
//! the first handler that recognises it answers. A handler that does not know a
//! request returns `None` and the next one gets a chance. That is what lets a
//! plugin answer a command the core never heard of, without the core learning
//! about plugins.
//!
//! The chain also owns the blocking classification, so the connection loop no
//! longer keeps a second `match` over the same requests. A request that reads
//! `/etc`, shells out or walks `/proc` is marked blocking where the handler that
//! serves it lives, not in a separate list that drifts.

use anth_proto::{Request, Response};

use crate::accounts;
use crate::accounts_admin;
use crate::config::FilesConfig;
use crate::daemon::Policy;
use crate::files::FileService;
use crate::firewall;
use crate::hostinfo;
use crate::network;
use crate::process;
use crate::volumes;

/// Context a handler needs: deployment policy plus the services owned by the
/// daemon process rather than by one connection.
pub struct Dispatch<'a> {
    pub policy: &'a Policy,
    pub files: &'a FileService,
}

/// One link of the chain.
///
/// `handle` is synchronous on purpose: the connection loop runs it on a blocking
/// worker when `blocks` says so, and inline otherwise. A handler that needs to
/// wait for a session or an upload is stateful and lives in the connection loop
/// instead; those requests answer `None` here.
pub trait Handler: Send + Sync {
    fn handle(&self, request: &Request, context: &Dispatch<'_>) -> Option<Response>;

    /// Whether serving this request may block. The default keeps a handler that
    /// forgot to answer on the cheap path, which is the safe direction: a
    /// needless `spawn_blocking` costs a thread, a missing one stalls every
    /// client of the connection.
    fn blocks(&self, _request: &Request) -> bool {
        false
    }
}

/// Runs a request through every handler in order and returns the first answer.
///
/// A request no handler recognises is a protocol error, not a panic: an older
/// daemon and a newer client have to fail with a message the operator can read.
pub fn dispatch(chain: &[&dyn Handler], request: &Request, context: &Dispatch<'_>) -> Response {
    for handler in chain {
        if let Some(response) = handler.handle(request, context) {
            return response;
        }
    }

    Response::Error {
        detail: format!("no handler accepted this request: {request:?}"),
    }
}

/// Whether any handler in the chain wants this request on a blocking worker.
pub fn blocks(chain: &[&dyn Handler], request: &Request) -> bool {
    chain.iter().any(|handler| handler.blocks(request))
}

/// The built-in handler: every command the panel implements itself.
pub struct CoreHandler;

impl Handler for CoreHandler {
    fn handle(&self, request: &Request, context: &Dispatch<'_>) -> Option<Response> {
        Some(handle_core(request, context.policy, context.files))
    }

    fn blocks(&self, request: &Request) -> bool {
        // Terminal and upload requests are stateful and never reach this
        // handler, so everything left either walks a filesystem, reads `/proc`,
        // reads `/etc` or runs a privileged command. Echo is the only answer
        // that touches nothing.
        !matches!(request, Request::Echo { .. })
    }
}

fn handle_core(request: &Request, policy: &Policy, files: &FileService) -> Response {
    match request {
        Request::Echo { message } => Response::Ok {
            message: message.clone(),
        },
        Request::HostInfo => Response::HostInfo(Box::new(hostinfo::collect())),
        Request::ProcessList => Response::ProcessSnapshot(process::collect(policy.process_control)),
        Request::ProcessSignal { pid, signal } => {
            match process::send_signal(*pid, *signal, policy.process_control) {
                Ok(()) => Response::ProcessSignalled {
                    pid: *pid,
                    signal: *signal,
                },
                Err(error) => Response::Error {
                    detail: error.detail(),
                },
            }
        }
        Request::NetworkList => Response::NetworkSnapshot(network::collect(policy.network_control)),
        Request::NetworkSetState { name, up } => {
            match network::set_state(policy.network_control, policy.sudo, name, *up) {
                Ok(()) => Response::NetworkStateChanged {
                    name: name.clone(),
                    up: *up,
                },
                Err(error) => Response::NetworkError(error),
            }
        }
        Request::FirewallStatus => match firewall::status(policy.firewall_control, policy.sudo) {
            Ok(snapshot) => Response::FirewallSnapshot(snapshot),
            Err(error) => Response::FirewallError(error),
        },
        Request::FirewallAddRule {
            action,
            port,
            protocol,
            comment,
        } => match firewall::add_rule(
            policy.firewall_control,
            policy.sudo,
            *action,
            *port,
            *protocol,
            comment.as_deref(),
        ) {
            Ok(message) => Response::FirewallChanged { message },
            Err(error) => Response::FirewallError(error),
        },
        Request::FirewallDeleteRule { handle } => {
            match firewall::delete_rule(policy.firewall_control, policy.sudo, *handle) {
                Ok(message) => Response::FirewallChanged { message },
                Err(error) => Response::FirewallError(error),
            }
        }
        Request::FirewallFlush => match firewall::flush(policy.firewall_control, policy.sudo) {
            Ok(message) => Response::FirewallChanged { message },
            Err(error) => Response::FirewallError(error),
        },
        Request::VolumeList => Response::VolumeSnapshot(volumes::collect(policy.volume_control)),
        Request::VolumeMount {
            device,
            target,
            fs_type,
            read_only,
            allow_removable,
        } => {
            let args = volumes::MountArgs {
                device,
                target,
                fs_type: fs_type.as_deref(),
                read_only: *read_only,
                allow_removable: *allow_removable,
            };
            match volumes::mount(
                policy.volume_control,
                policy.sudo,
                policy.mount_root.as_deref(),
                &args,
            ) {
                Ok(message) => Response::VolumeChanged { message },
                Err(error) => Response::VolumeError(error),
            }
        }
        Request::VolumeUnmount { target } => match volumes::unmount(
            policy.volume_control,
            policy.sudo,
            &policy.protected_paths,
            target,
        ) {
            Ok(message) => Response::VolumeChanged { message },
            Err(error) => Response::VolumeError(error),
        },
        Request::AccountList => Response::AccountSnapshot(accounts::collect(&policy.accounts)),
        Request::AccountCreate {
            name,
            uid,
            gid,
            gecos,
            home,
            shell,
            system,
        } => handle_account(policy, format!("created user {name}"), |guard| {
            accounts_admin::plan_create(
                guard,
                &accounts_admin::CreateArgs {
                    name,
                    uid: *uid,
                    gid: *gid,
                    gecos: gecos.as_deref(),
                    home: home.as_deref(),
                    shell: shell.as_deref(),
                    system: *system,
                },
            )
            .map(|call| vec![call])
        }),
        Request::AccountModify {
            name,
            gecos,
            shell,
            home,
            primary_gid,
            locked,
        } => handle_account(policy, format!("modified user {name}"), |guard| {
            accounts_admin::plan_modify(
                guard,
                &accounts_admin::ModifyArgs {
                    name,
                    gecos: gecos.as_deref(),
                    shell: shell.as_deref(),
                    home: home.as_deref(),
                    primary_gid: *primary_gid,
                    locked: *locked,
                },
            )
            .map(|call| vec![call])
        }),
        Request::AccountDelete { name, remove_home } => {
            handle_account(policy, format!("deleted user {name}"), |guard| {
                accounts_admin::plan_delete(guard, name, *remove_home).map(|call| vec![call])
            })
        }
        Request::AccountSetPassword { name, password } => {
            handle_account(policy, format!("password updated for {name}"), |guard| {
                accounts_admin::plan_set_password(
                    guard,
                    policy.accounts.manage_passwords,
                    name,
                    password.expose(),
                )
                .map(|call| vec![call])
            })
        }
        Request::AccountGroupCreate { name, gid, system } => {
            handle_account(policy, format!("created group {name}"), |guard| {
                accounts_admin::plan_group_create(guard, name, *gid, *system).map(|call| vec![call])
            })
        }
        Request::AccountGroupModify { name, gid, members } => {
            handle_account(policy, format!("modified group {name}"), |guard| {
                accounts_admin::plan_group_modify(guard, name, *gid, members.as_deref())
            })
        }
        Request::AccountGroupDelete { name } => {
            handle_account(policy, format!("deleted group {name}"), |guard| {
                accounts_admin::plan_group_delete(guard, name).map(|call| vec![call])
            })
        }
        Request::FileList { path } => match files.list(path) {
            Ok(listing) => Response::FileListing(listing),
            Err(error) => Response::FileError(error),
        },
        Request::FileRead { path, offset, len } => match files.read(path, *offset, *len) {
            Ok(chunk) => Response::FileChunk(chunk),
            Err(error) => Response::FileError(error),
        },
        Request::FileMkdir { path, recursive } => match files.mkdir(path, *recursive) {
            Ok(path) => Response::FileCreated { path },
            Err(error) => Response::FileError(error),
        },
        Request::FileRename { from, to } => match files.rename(from, to) {
            Ok((from, to)) => Response::FileRenamed { from, to },
            Err(error) => Response::FileError(error),
        },
        Request::FileDelete { path, recursive } => match files.delete(path, *recursive) {
            Ok(path) => Response::FileDeleted { path },
            Err(error) => Response::FileError(error),
        },
        // Stateful requests belong to a live connection. Reaching them here
        // means a caller used the line helper outside a session.
        Request::FileWriteStart { .. }
        | Request::FileWriteChunk { .. }
        | Request::FileWriteFinish { .. }
        | Request::FileWriteAbort { .. }
        | Request::TerminalOpen(_)
        | Request::TerminalInput { .. }
        | Request::TerminalResize { .. }
        | Request::TerminalClose { .. } => Response::Error {
            detail: "this request requires an open connection".to_string(),
        },
    }
}

/// Validates a mutation against the unfitted inventory, then runs it.
///
/// The snapshot is read once per request, unfitted, so a guard never mistakes a
/// truncated list for a missing account. `plan` returns every call the request
/// needs, which is more than one when a group changes both its gid and its
/// members.
fn handle_account(
    policy: &Policy,
    message: String,
    plan: impl FnOnce(
        &accounts_admin::Guard<'_>,
    ) -> Result<Vec<accounts_admin::PlannedCall>, anth_proto::AccountError>,
) -> Response {
    if !policy.accounts.control {
        return Response::AccountError(anth_proto::AccountError::new(
            anth_proto::AccountErrorKind::Disabled,
            "account control is disabled",
        ));
    }

    let (snapshot, _, ranges) =
        accounts::collect_detailed(&policy.accounts, &accounts::SystemProbe::new());
    let guard =
        accounts_admin::Guard::new(&snapshot, &policy.accounts, policy.daemon_user.as_str())
            .with_id_ranges(ranges);

    let calls = match plan(&guard) {
        Ok(calls) => calls,
        Err(error) => return Response::AccountError(error),
    };

    let runner = accounts_admin::system_runner(policy.sudo);
    for call in &calls {
        if let Err(error) = accounts_admin::run_planned(&runner, call) {
            return Response::AccountError(error);
        }
    }
    Response::AccountChanged { message }
}

/// The policy the line helper and unit tests use. Kept here so `daemon` does not
/// need to know how the chain is assembled.
pub fn default_files() -> FileService {
    FileService::new(&FilesConfig::default()).expect("the default filesystem root must be usable")
}
