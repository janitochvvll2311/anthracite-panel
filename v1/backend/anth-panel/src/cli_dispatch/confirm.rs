use super::super::main_prelude::*;

pub fn confirm_account_password(args: &AccountUserPasswordArgs) -> Result<bool, String> {
    confirm_action(
        args.yes,
        || format!("Set the password for `{}`? [y/N] ", args.name),
        "refusing to change a password without --yes when stdin is not a terminal",
    )
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
}

pub fn confirm_account_group_delete(args: &AccountGroupDeleteArgs) -> Result<bool, String> {
    confirm_action(
        args.yes,
        || format!("Delete group `{}`? [y/N] ", args.name),
        "refusing to delete a group without --yes when stdin is not a terminal",
    )
}

pub fn confirm_account_group_modify(args: &AccountGroupModifyArgs) -> Result<bool, String> {
    confirm_action(
        args.yes,
        || format!("Replace the members of `{}`? [y/N] ", args.name),
        "refusing to change group membership without --yes when stdin is not a terminal",
    )
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

/// Turns a confirmation into the process exit code the CLI should return.
///
/// `None` means the action may proceed. A declined prompt exits `0` because the
/// user chose it, and a refusal exits `1`. Every subcommand that asks goes
/// through here so the two messages and the two codes stay in one place.
pub fn gate(outcome: Result<bool, String>) -> Option<ExitCode> {
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

    gate(outcome)
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

/// A partition edit destroys data on a device the operator had to name, so the
/// confirmation repeats the disk and the size rather than asking a yes/no that
/// reads the same for every command.
pub fn confirm_partition_create(args: &PartitionCreateCli) -> Result<bool, String> {
    confirm_action(
        args.yes,
        || {
            format!(
                "Create a {} partition of {} bytes on `{}`? This rewrites the partition table. [y/N] ",
                args.kind, args.size, args.disk
            )
        },
        "refusing to create a partition without --yes when stdin is not a terminal",
    )
}

pub fn confirm_partition_resize(args: &PartitionResizeCli) -> Result<bool, String> {
    confirm_action(
        args.yes,
        || {
            format!(
                "Resize partition {} on `{}` to {} bytes? [y/N] ",
                args.number, args.disk, args.size
            )
        },
        "refusing to resize a partition without --yes when stdin is not a terminal",
    )
}

pub fn confirm_partition_delete(args: &PartitionDeleteCli) -> Result<bool, String> {
    confirm_action(
        args.yes,
        || {
            format!(
                "Delete partition {} on `{}`? [y/N] ",
                args.number, args.disk
            )
        },
        "refusing to delete a partition without --yes when stdin is not a terminal",
    )
}

pub fn confirm_partition_format(args: &PartitionFormatCli) -> Result<bool, String> {
    confirm_action(
        args.yes,
        || {
            format!(
                "Format `{}` as {}? Everything on it is lost. [y/N] ",
                args.partition, args.fs_type
            )
        },
        "refusing to format a partition without --yes when stdin is not a terminal",
    )
}
