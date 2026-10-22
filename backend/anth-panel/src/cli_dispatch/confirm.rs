use super::super::main_prelude::*;

pub fn fail_confirmation(error: String) -> ExitCode {
    eprintln!("[ERROR] {error}");
    ExitCode::FAILURE
}

/// Prints the cancellation notice once, for every account subcommand that asks.
pub fn report_cancel(confirmed: bool) -> bool {
    if !confirmed {
        eprintln!("[cancelled]");
    }
    confirmed
}

pub fn confirm_account_password(args: &AccountUserPasswordArgs) -> Result<bool, String> {
    confirm_action(
        args.yes,
        || format!("Set the password for `{}`? [y/N] ", args.name),
        "refusing to change a password without --yes when stdin is not a terminal",
    )
    .map(report_cancel)
}

pub fn confirm_account_user_delete(args: &AccountUserDeleteArgs) -> Result<bool, String> {
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

pub fn confirm_account_group_delete(args: &AccountGroupDeleteArgs) -> Result<bool, String> {
    confirm_action(
        args.yes,
        || format!("Delete group `{}`? [y/N] ", args.name),
        "refusing to delete a group without --yes when stdin is not a terminal",
    )
    .map(report_cancel)
}

pub fn confirm_account_group_modify(args: &AccountGroupModifyArgs) -> Result<bool, String> {
    confirm_action(
        args.yes,
        || format!("Replace the members of `{}`? [y/N] ", args.name),
        "refusing to change group membership without --yes when stdin is not a terminal",
    )
    .map(report_cancel)
}

/// Confirmation for every account subcommand that changes or removes data.
/// `None` covers the read-only and additive ones, which need no prompt.
pub fn account_confirmation(command: &Command) -> Option<Result<bool, String>> {
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

pub fn confirm_action(
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

pub fn confirm_signal(args: &ProcessSignalArgs) -> Result<bool, String> {
    confirm_action(
        args.yes,
        || format!("Send {} to process {}? [y/N] ", args.signal, args.pid),
        "refusing to signal without --yes when stdin is not a terminal",
    )
}

pub fn confirm_spawn(args: &ProcessSpawnArgs) -> Result<bool, String> {
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
pub fn confirm_network_manage(command: &NetworkCommand) -> Option<ExitCode> {
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

pub fn kind_name(kind: NetworkKindArg) -> &'static str {
    match kind {
        NetworkKindArg::Veth => "veth",
        NetworkKindArg::Bridge => "bridge",
        NetworkKindArg::Bond => "bond",
        NetworkKindArg::Vlan => "vlan",
        NetworkKindArg::Dummy => "dummy",
        NetworkKindArg::TunTap => "tun/tap",
    }
}

pub fn confirm_network_state(args: &NetworkStateArgs, up: bool) -> Result<bool, String> {
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

pub fn confirm_firewall_add(
    args: &FirewallAddArgs,
    action: FirewallAction,
) -> Result<bool, String> {
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

pub fn confirm_firewall_delete(args: &FirewallDeleteArgs) -> Result<bool, String> {
    confirm_action(
        args.yes,
        || format!("Delete firewall rule {}? [y/N] ", args.handle),
        "refusing to change the firewall without --yes when stdin is not a terminal",
    )
}

pub fn confirm_firewall_flush(args: &FirewallFlushArgs) -> Result<bool, String> {
    confirm_action(
        args.yes,
        || "Flush every rule from the managed firewall chain? [y/N] ".to_string(),
        "refusing to change the firewall without --yes when stdin is not a terminal",
    )
}

pub fn confirm_delete(args: &FileDeleteArgs) -> Result<bool, String> {
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

pub fn confirm_trash_restore(args: &TrashRestoreArgs) -> Result<bool, String> {
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

pub fn confirm_trash_empty(args: &TrashEmptyArgs) -> Result<bool, String> {
    confirm_action(
        args.yes,
        || "Empty the trash? This cannot be undone. [y/N] ".to_string(),
        "refusing to empty the trash without --yes when stdin is not a terminal",
    )
}

pub fn confirm_volume_mount(args: &VolumeMountArgs) -> Result<bool, String> {
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

pub fn confirm_volume_unmount(args: &VolumeUnmountArgs) -> Result<bool, String> {
    confirm_action(
        args.yes,
        || format!("Unmount `{}`? [y/N] ", args.target),
        "refusing to unmount without --yes when stdin is not a terminal",
    )
}
