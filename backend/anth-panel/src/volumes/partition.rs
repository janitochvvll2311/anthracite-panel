//! Partition table edits: create, resize, delete and format.
//!
//! Every operation is planned into a `PlannedCall` — one program plus an
//! argument vector — before anything runs. Nothing here builds a shell string,
//! so a device name, a label or a size can never become a second command.
//!
//! The guards live in `allowed_disk`, `refuse_protected_disk` and
//! `refuse_busy_disk`, and they run against a fresh snapshot. A partition table
//! is the one thing in this panel where a mistake is not undone by unmounting:
//! `sfdisk` rewrites the table, and the wrong disk number destroys a
//! filesystem the request never mentioned.

use std::path::{Component, Path, PathBuf};

use anth_proto::{
    PartitionCreateArgs, PartitionEntry, PartitionError, PartitionSnapshot, PartitionTableAction,
    VolumeDevice, VolumeErrorKind, VolumeKind, VolumeSnapshot,
};

use crate::command::{CommandError, CommandOutput, CommandRunner, SystemRunner};
use crate::volumes::{SystemProbe, collect_with};

/// Filesystems `mkfs` may create. The type selects the program, so it is matched
/// against this list and never interpolated into an argument.
pub const FORMAT_TYPES: &[&str] = &["ext4", "xfs", "vfat", "exfat", "btrfs", "swap"];

/// One program and its arguments, ready to run.
///
/// `Debug` is derived, including `stdin`, because no field here is a secret: a
/// partition script names a size, not a credential. The script still travels on
/// stdin rather than argv so a long table does not show up in `/proc`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedCall {
    pub program: &'static str,
    pub argv: Vec<String>,
    pub stdin: Option<String>,
}

impl PlannedCall {
    fn argv_refs(&self) -> Vec<&str> {
        self.argv.iter().map(String::as_str).collect()
    }
}

/// A closed enum, so the `sfdisk` script line is built from values the daemon
/// validated rather than from client text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableEdit {
    Create {
        kind: PartitionKind,
        start_bytes: u64,
        size_bytes: u64,
    },
    Resize {
        number: u32,
        size_bytes: u64,
    },
    Delete {
        number: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartitionKind {
    Linux,
    Swap,
    Efi,
}

impl PartitionKind {
    fn script_type(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::Swap => "swap",
            Self::Efi => "uefi",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "linux" => Some(Self::Linux),
            "swap" => Some(Self::Swap),
            "efi" => Some(Self::Efi),
            _ => None,
        }
    }
}

fn invalid(detail: impl Into<String>) -> PartitionError {
    PartitionError::new(VolumeErrorKind::Invalid, detail)
}

fn protected(detail: impl Into<String>) -> PartitionError {
    PartitionError::new(VolumeErrorKind::Protected, detail)
}

fn disabled(detail: impl Into<String>) -> PartitionError {
    PartitionError::new(VolumeErrorKind::Disabled, detail)
}

fn not_found(detail: impl Into<String>) -> PartitionError {
    PartitionError::new(VolumeErrorKind::NotFound, detail)
}

fn busy(detail: impl Into<String>) -> PartitionError {
    PartitionError::new(VolumeErrorKind::Busy, detail)
}

/// A device path is accepted only when absolute, with no `..` or `.` component.
/// `/dev/sda` passes; `../sda` and `/dev/../sda` do not.
pub fn validate_device_path(path: &str) -> Result<PathBuf, PartitionError> {
    if path.is_empty() {
        return Err(invalid("a device path is required"));
    }
    if !path.starts_with('/') {
        return Err(invalid(format!("`{path}` is not an absolute path")));
    }
    // `Path::components()` normalises `.` away and collapses a double slash, so
    // the raw segments are checked as well: `/dev/./sda` must not slip through
    // as `/dev/sda`.
    // The leading separator is stripped exactly once: `//dev/sda` then leaves an
    // empty segment, which is a doubled separator and is refused.
    let relative = path.strip_prefix('/').unwrap_or(path);
    for segment in relative.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(invalid(format!(
                "`{path}` contains a path component the panel refuses"
            )));
        }
    }
    for component in Path::new(path).components() {
        if !matches!(component, Component::Normal(_) | Component::RootDir) {
            return Err(invalid(format!(
                "`{path}` contains a path component the panel refuses"
            )));
        }
    }
    Ok(PathBuf::from(path))
}

/// Resolves one disk against the allowlist.
///
/// An empty allowlist denies every disk, the same rule `[process] spawn_allow`
/// follows: the list, not the switch, is the authority.
pub fn allowed_disk<'a>(
    snapshot: &'a VolumeSnapshot,
    allowed: &[String],
    path: &str,
) -> Result<&'a VolumeDevice, PartitionError> {
    validate_device_path(path)?;
    if allowed.is_empty() {
        return Err(protected(
            "no disk is listed in `[volumes] disk_allow`, so every edit is denied",
        ));
    }
    let device = snapshot
        .devices
        .iter()
        .find(|device| device.path == path)
        .ok_or_else(|| not_found(format!("`{path}` is not a known device")))?;

    if device.kind != VolumeKind::Disk {
        return Err(invalid(format!(
            "`{path}` is a {} and not a whole disk",
            device.kind.as_str()
        )));
    }
    if !allowed.iter().any(|listed| listed == &device.path) {
        return Err(protected(format!(
            "`{path}` is not listed in `[volumes] disk_allow`"
        )));
    }
    Ok(device)
}

/// Refuses a disk carrying a mount the panel itself depends on.
///
/// A table rewrite on the disk holding `/`, `/boot`, the config or the socket is
/// unrecoverable. The kernel would report `EBUSY` for `/`, but not for a
/// separate `/boot` that the guard knows about and the kernel does not.
pub fn refuse_protected_disk(
    snapshot: &VolumeSnapshot,
    disk: &VolumeDevice,
    protected_paths: &[PathBuf],
) -> Result<(), PartitionError> {
    for mount in &snapshot.mounts {
        if mount.mountpoint == "/" && mount_covers(mount.source.as_str(), disk) {
            return Err(protected(format!(
                "`{}` carries `/` and cannot be repartitioned",
                disk.path
            )));
        }
        if !mount_covers(mount.source.as_str(), disk) {
            continue;
        }
        if let Some(guarded) = protected_paths
            .iter()
            .find(|guarded| is_ancestor_or_equal(&mount.mountpoint, guarded))
        {
            return Err(protected(format!(
                "`{}` carries the mount on `{}`, which the panel needs to run",
                disk.path,
                guarded.display()
            )));
        }
    }
    Ok(())
}

/// True when `source` sits on `disk`.
///
/// The device tree is the authority, not a string prefix: `/dev/sdaa` starts
/// with `/dev/sda` and is a different disk, so a prefix test alone would refuse
/// an innocent device and, worse, could accept the wrong one. `lsblk` records
/// the parent, and the parent of a partition is the disk's kernel name.
fn mount_covers(source: &str, disk: &VolumeDevice) -> bool {
    if source == disk.path {
        return true;
    }
    let name = Path::new(source)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(source);
    name.starts_with(&disk.name)
        && name.get(disk.name.len()..).is_some_and(|rest| {
            !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit() || c == 'p')
        })
}

fn is_ancestor_or_equal(mountpoint: &str, guarded: &Path) -> bool {
    let mountpoint = Path::new(mountpoint);
    mountpoint == guarded || guarded.starts_with(mountpoint)
}

/// Refuses an edit while any partition of the disk is mounted.
///
/// `sfdisk` fails on a busy table anyway, but it does so with a kernel message
/// after the request was accepted. Refusing first names the offending mount.
pub fn refuse_busy_disk(
    snapshot: &VolumeSnapshot,
    disk: &VolumeDevice,
) -> Result<(), PartitionError> {
    for mount in &snapshot.mounts {
        if mount_covers(mount.source.as_str(), disk) {
            return Err(busy(format!(
                "`{}` has a partition mounted on `{}`; unmount it first",
                disk.path, mount.mountpoint
            )));
        }
    }
    Ok(())
}

/// Builds the `sfdisk` call for one edit.
///
/// The script goes on stdin. In argv it would be visible in `/proc`, and a size
/// field would become a place to smuggle text.
pub fn plan_edit(disk: &str, edit: &TableEdit) -> Result<PlannedCall, PartitionError> {
    let disk = validate_device_path(disk)?;
    let disk = disk
        .to_str()
        .ok_or_else(|| invalid("the device path is not valid UTF-8"))?;

    let script = match edit {
        TableEdit::Create {
            kind,
            start_bytes,
            size_bytes,
        } => {
            if *size_bytes == 0 {
                return Err(invalid("a new partition needs a size"));
            }
            let size = sectors(*size_bytes)?;
            // `start=0` means "let sfdisk place it", which is what a client that
            // did not read the free space wants.
            if *start_bytes == 0 {
                format!("size={size},type={}", kind.script_type())
            } else {
                let start = sectors(*start_bytes)?;
                format!("start={start},size={size},type={}", kind.script_type())
            }
        }
        TableEdit::Resize { number, size_bytes } => {
            if *number == 0 {
                return Err(invalid("partition numbers start at 1"));
            }
            if *size_bytes == 0 {
                return Err(invalid("resizing needs a size"));
            }
            format!("{number},size={}", sectors(*size_bytes)?)
        }
        TableEdit::Delete { number } => {
            if *number == 0 {
                return Err(invalid("partition numbers start at 1"));
            }
            format!("delete {number}")
        }
    };

    Ok(PlannedCall {
        program: "sfdisk",
        argv: vec!["--no-reread".to_string(), disk.to_string()],
        stdin: Some(script),
    })
}

/// A request that is not a multiple of 512 bytes is rejected rather than
/// rounded: a partition 511 bytes short of what was asked for is a bug the
/// operator did not ask for.
fn sectors(bytes: u64) -> Result<u64, PartitionError> {
    if !bytes.is_multiple_of(512) {
        return Err(invalid(format!(
            "{bytes} bytes is not a whole number of 512-byte sectors"
        )));
    }
    Ok(bytes / 512)
}

/// Builds the `mkfs` call. `swap` is `mkswap` and takes no `-t`, so the two are
/// built separately instead of being special-cased inside one flat argv.
pub fn plan_format(
    partition: &str,
    fs_type: &str,
    label: Option<&str>,
) -> Result<PlannedCall, PartitionError> {
    let partition = validate_device_path(partition)?;
    let partition = partition
        .to_str()
        .ok_or_else(|| invalid("the partition path is not valid UTF-8"))?;

    if !FORMAT_TYPES.contains(&fs_type) {
        return Err(invalid(format!(
            "`{fs_type}` is not one of {}",
            FORMAT_TYPES.join(", ")
        )));
    }
    if let Some(label) = label {
        if label.is_empty() || label.len() > 32 {
            return Err(invalid("a label needs 1 to 32 characters"));
        }
        if !label
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        {
            return Err(invalid(
                "a label may only contain letters, digits, `-`, `_` and `.`",
            ));
        }
    }

    let program = match fs_type {
        "swap" => "mkswap",
        "ext4" => "mkfs.ext4",
        "xfs" => "mkfs.xfs",
        "vfat" => "mkfs.vfat",
        "exfat" => "mkfs.exfat",
        "btrfs" => "mkfs.btrfs",
        other => return Err(invalid(format!("`{other}` has no mkfs program"))),
    };

    let mut argv = Vec::new();
    if let Some(label) = label {
        argv.push("-L".to_string());
        argv.push(label.to_string());
    }
    argv.push(partition.to_string());

    Ok(PlannedCall {
        program,
        argv,
        stdin: None,
    })
}

/// Runs a planned call. Every partition edit lands here, so the error mapping
/// exists once.
pub fn execute(runner: &dyn CommandRunner, call: &PlannedCall) -> Result<(), PartitionError> {
    let outcome = match call.stdin.as_deref() {
        Some(script) => runner.run_with_stdin(call.program, &call.argv_refs(), script),
        None => runner.run(call.program, &call.argv_refs()),
    };
    match outcome {
        Ok(CommandOutput { ok: true, .. }) => Ok(()),
        Ok(output) => Err(PartitionError::new(
            VolumeErrorKind::Failed,
            first_line(&output.stderr, call.program),
        )),
        Err(CommandError::Unavailable { .. }) => Err(PartitionError::new(
            VolumeErrorKind::Failed,
            format!("{} is not installed or is not on PATH", call.program),
        )),
        Err(CommandError::Failed { detail, .. }) => Err(PartitionError::new(
            VolumeErrorKind::Failed,
            format!("could not run {}: {detail}", call.program),
        )),
    }
}

fn first_line(stderr: &str, program: &str) -> String {
    let line = stderr
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("the command failed without a message");
    format!("{program}: {line}")
}

/// Runs one edit against a snapshot.
///
/// The guard order is the point of this function: resolve the disk, refuse the
/// protected one, refuse a busy one, plan, then execute. A caller cannot skip a
/// step because there is no other entry point.
pub fn edit_with(
    runner: &dyn CommandRunner,
    snapshot: &VolumeSnapshot,
    action: &PartitionTableAction,
    allowed_disks: &[String],
    protected_paths: &[PathBuf],
    format_enabled: bool,
) -> Result<String, PartitionError> {
    match action {
        PartitionTableAction::Resize {
            disk,
            number,
            size_bytes,
        } => {
            let device = allowed_disk(snapshot, allowed_disks, disk)?;
            refuse_protected_disk(snapshot, device, protected_paths)?;
            refuse_busy_disk(snapshot, device)?;
            let call = plan_edit(
                &device.path,
                &TableEdit::Resize {
                    number: *number,
                    size_bytes: *size_bytes,
                },
            )?;
            execute(runner, &call)?;
            Ok(format!("resized partition {number} on {}", device.path))
        }
        PartitionTableAction::Delete { disk, number } => {
            let device = allowed_disk(snapshot, allowed_disks, disk)?;
            refuse_protected_disk(snapshot, device, protected_paths)?;
            refuse_busy_disk(snapshot, device)?;
            let call = plan_edit(&device.path, &TableEdit::Delete { number: *number })?;
            execute(runner, &call)?;
            Ok(format!("deleted partition {number} on {}", device.path))
        }
        PartitionTableAction::Format {
            partition,
            fs_type,
            label,
        } => {
            if !format_enabled {
                return Err(disabled("`[volumes] format` is off"));
            }
            // A format needs a partition that exists and is not in use. The
            // allowlist covers disks, not partitions, so the partition is
            // resolved through its disk: a partition whose disk is not listed
            // is refused for the same reason its disk is.
            let known = snapshot
                .devices
                .iter()
                .find(|device| device.path == *partition)
                .ok_or_else(|| not_found(format!("`{partition}` is not a known device")))?;
            if known.kind != VolumeKind::Part {
                return Err(invalid(format!(
                    "`{partition}` is a {} and not a partition",
                    known.kind.as_str()
                )));
            }
            let parent = known
                .parent
                .as_deref()
                .ok_or_else(|| invalid(format!("`{partition}` has no parent disk")))?;
            let disk = snapshot
                .devices
                .iter()
                .find(|device| device.kind == VolumeKind::Disk && device.name == parent)
                .ok_or_else(|| not_found(format!("the disk behind `{partition}` is unknown")))?;
            allowed_disk(snapshot, allowed_disks, &disk.path)?;

            if let Some(mount) = snapshot
                .mounts
                .iter()
                .find(|mount| mount.source == *partition)
            {
                return Err(busy(format!(
                    "`{partition}` is mounted on `{}`; unmount it before formatting",
                    mount.mountpoint
                )));
            }
            let call = plan_format(partition, fs_type, label.as_deref())?;
            execute(runner, &call)?;
            Ok(format!("formatted {partition} as {fs_type}"))
        }
    }
}

/// Collects a snapshot and runs one edit against it.
pub fn perform(
    partition_enabled: bool,
    format_enabled: bool,
    sudo_enabled: bool,
    allowed_disks: &[String],
    protected_paths: &[PathBuf],
    action: &PartitionTableAction,
) -> Result<String, PartitionError> {
    if !partition_enabled {
        return Err(disabled("`[volumes] partition` is off"));
    }
    let probe = SystemProbe::new();
    let snapshot = collect_with(partition_enabled, &probe);
    let runner = SystemRunner::new(sudo_enabled);
    edit_with(
        &runner,
        &snapshot,
        action,
        allowed_disks,
        protected_paths,
        format_enabled,
    )
}

/// Creates a partition under the daemon policy.
pub fn perform_create(
    policy: &crate::daemon::Policy,
    args: &PartitionCreateArgs,
) -> Result<String, PartitionError> {
    if !policy.volume_partition {
        return Err(disabled("`[volumes] partition` is off"));
    }
    let probe = SystemProbe::new();
    let snapshot = collect_with(policy.volume_control, &probe);
    let runner = SystemRunner::new(policy.sudo);
    create_with(
        &runner,
        &snapshot,
        args,
        &policy.disk_allow,
        &policy.protected_paths,
    )
}

/// Creates a partition, from a request to an executed call.
pub fn create_with(
    runner: &dyn CommandRunner,
    snapshot: &VolumeSnapshot,
    args: &PartitionCreateArgs,
    allowed_disks: &[String],
    protected_paths: &[PathBuf],
) -> Result<String, PartitionError> {
    let device = allowed_disk(snapshot, allowed_disks, &args.disk)?;
    refuse_protected_disk(snapshot, device, protected_paths)?;
    refuse_busy_disk(snapshot, device)?;

    let kind = PartitionKind::parse(&args.kind)
        .ok_or_else(|| invalid(format!("`{}` is not a partition kind", args.kind)))?;
    let call = plan_edit(
        &device.path,
        &TableEdit::Create {
            kind,
            start_bytes: args.start_bytes,
            size_bytes: args.size_bytes,
        },
    )?;
    execute(runner, &call)?;
    Ok(format!(
        "created a {} partition of {} bytes on {}",
        args.kind, args.size_bytes, device.path
    ))
}

/// The partition table of one disk, as the panel reports it.
pub fn snapshot_for(
    snapshot: &VolumeSnapshot,
    disk: &VolumeDevice,
    control_enabled: bool,
    format_enabled: bool,
    allowed_disks: &[String],
    warning: Option<String>,
) -> PartitionSnapshot {
    PartitionSnapshot {
        disk: disk.path.clone(),
        partitions: partitions_on(snapshot, disk),
        control_enabled,
        format_enabled,
        allowed_disks: allowed_disks.to_vec(),
        format_types: FORMAT_TYPES.iter().map(|name| name.to_string()).collect(),
        warning,
    }
}

/// The partitions of one disk, in table order.
pub fn partitions_on(snapshot: &VolumeSnapshot, disk: &VolumeDevice) -> Vec<PartitionEntry> {
    let mut entries: Vec<PartitionEntry> = snapshot
        .devices
        .iter()
        .filter(|device| {
            device.kind == VolumeKind::Part && device.parent.as_deref() == Some(&disk.name)
        })
        .map(|device| PartitionEntry {
            number: partition_number(&device.name).unwrap_or(0),
            path: device.path.clone(),
            size_bytes: device.size_bytes,
            fstype: device.fstype.clone(),
            mountpoints: device.mountpoints.clone(),
        })
        .collect();
    entries.sort_by_key(|entry| entry.number);
    entries
}

/// `/dev/sda3` and `/dev/nvme0n1p3` both end in the partition number, so the
/// trailing digits are read instead of splitting on a separator that differs
/// between disk naming schemes.
pub fn partition_number(name: &str) -> Option<u32> {
    let digits: String = name
        .chars()
        .rev()
        .take_while(char::is_ascii_digit)
        .collect();
    if digits.is_empty() {
        return None;
    }
    digits.chars().rev().collect::<String>().parse().ok()
}
