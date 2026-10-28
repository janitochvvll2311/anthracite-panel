//! Partition-table edits and filesystem formatting.
//!
//! Every operation is planned into one program plus argv before anything runs.
//! `sfdisk` scripts travel on stdin, so no size or label can become a second
//! command. The guards run against a fresh snapshot: the disk must be in
//! `disk_allow`, must not carry a mount the panel needs, must not be busy, and
//! must not hold a whole-disk filesystem.

use std::path::{Component, Path, PathBuf};

use anth_wire::{
    ErrorKind, PartitionKind, VolumeDevice, VolumeFormatRequest, VolumeKind, VolumePartitionCreate,
    VolumePartitionDelete, VolumePartitionResize, VolumeSnapshot, WireError,
};

use super::{MountOutput, SystemVolumes};

/// Filesystems `mkfs` may create. The type selects the program, so it is
/// matched against this list and never interpolated into an argv element.
pub const FORMAT_TYPES: &[&str] = &["ext4", "xfs", "vfat", "exfat", "btrfs", "swap"];

/// One program and its arguments, ready to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedCall {
    pub program: &'static str,
    pub argv: Vec<String>,
    pub stdin: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TableEdit {
    Create {
        kind: PartitionKind,
        start_bytes: u64,
        size_bytes: u64,
    },
}

fn disabled(kind: &'static str, message: &str) -> WireError {
    WireError::new(ErrorKind::Forbidden, kind, message)
}

fn invalid(message: impl Into<String>) -> WireError {
    WireError::new(ErrorKind::Invalid, "invalid_partition", message)
}

fn protected(message: impl Into<String>) -> WireError {
    WireError::new(ErrorKind::Forbidden, "protected_disk", message)
}

fn not_found(message: impl Into<String>) -> WireError {
    WireError::new(ErrorKind::NotFound, "partition_not_found", message)
}

fn busy(message: impl Into<String>) -> WireError {
    WireError::new(ErrorKind::Conflict, "disk_busy", message)
}

/// A device path is accepted only when absolute, with no `.` or `..` segment.
pub fn validate_device_path(path: &str) -> Result<PathBuf, WireError> {
    if path.is_empty() {
        return Err(invalid("a device path is required"));
    }
    if !path.starts_with('/') {
        return Err(invalid(format!("`{path}` is not an absolute path")));
    }
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

/// Resolves a whole disk against the allowlist. An empty allowlist denies
/// every edit; the list is the authority, not the switch.
pub fn allowed_disk<'a>(
    snapshot: &'a VolumeSnapshot,
    allowed: &[String],
    path: &str,
) -> Result<&'a VolumeDevice, WireError> {
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

pub fn refuse_protected_disk(
    snapshot: &VolumeSnapshot,
    disk: &VolumeDevice,
    protected_paths: &[PathBuf],
) -> Result<(), WireError> {
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

pub fn refuse_busy_disk(snapshot: &VolumeSnapshot, disk: &VolumeDevice) -> Result<(), WireError> {
    for mount in &snapshot.mounts {
        if mount_covers(mount.source.as_str(), disk) {
            return Err(busy(format!(
                "`{}` has a partition mounted on `{}`; unmount it first",
                disk.path, mount.mountpoint
            )));
        }
    }
    for swap in &snapshot.swap_devices {
        if mount_covers(swap.as_str(), disk) {
            return Err(busy(format!(
                "`{}` backs an active swap area; swapoff it first",
                disk.path
            )));
        }
    }
    Ok(())
}

pub fn refuse_active_swap(snapshot: &VolumeSnapshot, targets: &[&str]) -> Result<(), WireError> {
    for target in targets {
        if snapshot.swap_devices.iter().any(|swap| swap == target) {
            return Err(busy(format!(
                "`{target}` backs an active swap area; swapoff it first"
            )));
        }
    }
    Ok(())
}

pub fn refuse_whole_disk_filesystem(disk: &VolumeDevice) -> Result<(), WireError> {
    if let Some(fstype) = &disk.fstype
        && !fstype.is_empty()
        && fstype != "swap"
    {
        return Err(busy(format!(
            "`{}` holds a {fstype} filesystem outside any partition table; writing a table would destroy it",
            disk.path
        )));
    }
    Ok(())
}

/// `/dev/sda3` and `/dev/nvme0n1p3` both end in the partition number.
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

fn sectors(bytes: u64) -> Result<u64, WireError> {
    if bytes == 0 {
        return Err(invalid("a partition needs a size"));
    }
    if !bytes.is_multiple_of(512) {
        return Err(invalid(format!(
            "{bytes} bytes is not a whole number of 512-byte sectors"
        )));
    }
    Ok(bytes / 512)
}

fn plan_edit(disk: &str, edit: &TableEdit, has_table: bool) -> Result<PlannedCall, WireError> {
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
            let size = sectors(*size_bytes)?;
            if *start_bytes == 0 {
                format!("size={size},type={}", kind.script_type())
            } else {
                let start = sectors(*start_bytes)?;
                format!("start={start},size={size},type={}", kind.script_type())
            }
        }
    };
    let mut argv = vec!["--no-reread".to_string()];
    if has_table {
        argv.push("--append".to_string());
    }
    argv.push(disk.to_string());
    Ok(PlannedCall {
        program: "sfdisk",
        argv,
        stdin: Some(script),
    })
}

fn plan_resize(disk: &str, number: u32, size_bytes: u64) -> Result<PlannedCall, WireError> {
    let disk = validate_device_path(disk)?;
    let disk = disk
        .to_str()
        .ok_or_else(|| invalid("the device path is not valid UTF-8"))?;
    if number == 0 {
        return Err(invalid("partition numbers start at 1"));
    }
    Ok(PlannedCall {
        program: "sfdisk",
        argv: vec![
            "--no-reread".to_string(),
            "-N".to_string(),
            number.to_string(),
            disk.to_string(),
        ],
        stdin: Some(format!("size={}", sectors(size_bytes)?)),
    })
}

fn plan_delete(disk: &str, number: u32) -> Result<PlannedCall, WireError> {
    let disk = validate_device_path(disk)?;
    let disk = disk
        .to_str()
        .ok_or_else(|| invalid("the device path is not valid UTF-8"))?;
    if number == 0 {
        return Err(invalid("partition numbers start at 1"));
    }
    Ok(PlannedCall {
        program: "sfdisk",
        argv: vec![
            "--no-reread".to_string(),
            "--delete".to_string(),
            disk.to_string(),
            number.to_string(),
        ],
        stdin: None,
    })
}

fn plan_format(
    partition: &str,
    fs_type: &str,
    label: Option<&str>,
) -> Result<PlannedCall, WireError> {
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
    let max_label = if fs_type == "vfat" { 11 } else { 32 };
    if let Some(label) = label {
        if label.is_empty() || label.len() > max_label {
            return Err(invalid(format!(
                "a {fs_type} label needs 1 to {max_label} characters"
            )));
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
    let label_flag = if fs_type == "vfat" { "-n" } else { "-L" };
    let mut argv = Vec::new();
    if let Some(label) = label {
        argv.push(label_flag.to_string());
        argv.push(label.to_string());
    }
    argv.push(partition.to_string());
    Ok(PlannedCall {
        program,
        argv,
        stdin: None,
    })
}

impl SystemVolumes {
    pub fn create_partition(&self, args: &VolumePartitionCreate) -> Result<String, WireError> {
        if !self.config.partition {
            return Err(disabled(
                "volume_partition_disabled",
                "`[volumes] partition` is off",
            ));
        }
        let snapshot = self.snapshot();
        let device = allowed_disk(&snapshot, &self.config.disk_allow, &args.disk)?;
        refuse_protected_disk(&snapshot, device, &self.protected)?;
        refuse_whole_disk_filesystem(device)?;
        refuse_busy_disk(&snapshot, device)?;
        let has_table = snapshot
            .devices
            .iter()
            .any(|child| child.parent.as_deref() == Some(device.name.as_str()));
        let call = plan_edit(
            &device.path,
            &TableEdit::Create {
                kind: args.kind,
                start_bytes: args.start_bytes,
                size_bytes: args.size_bytes,
            },
            has_table,
        )?;
        self.run_plan(&call)?;
        Ok(format!(
            "created a {} partition of {} bytes on {}",
            args.kind, args.size_bytes, device.path
        ))
    }

    pub fn resize_partition(&self, args: &VolumePartitionResize) -> Result<String, WireError> {
        if !self.config.partition {
            return Err(disabled(
                "volume_partition_disabled",
                "`[volumes] partition` is off",
            ));
        }
        let snapshot = self.snapshot();
        let device = allowed_disk(&snapshot, &self.config.disk_allow, &args.disk)?;
        refuse_protected_disk(&snapshot, device, &self.protected)?;
        refuse_whole_disk_filesystem(device)?;
        refuse_busy_disk(&snapshot, device)?;
        require_partition(&snapshot, device, args.number)?;
        let call = plan_resize(&device.path, args.number, args.size_bytes)?;
        self.run_plan(&call)?;
        Ok(format!(
            "resized partition {} on {}",
            args.number, device.path
        ))
    }

    pub fn delete_partition(&self, args: &VolumePartitionDelete) -> Result<String, WireError> {
        if !self.config.partition {
            return Err(disabled(
                "volume_partition_disabled",
                "`[volumes] partition` is off",
            ));
        }
        let snapshot = self.snapshot();
        let device = allowed_disk(&snapshot, &self.config.disk_allow, &args.disk)?;
        refuse_protected_disk(&snapshot, device, &self.protected)?;
        refuse_whole_disk_filesystem(device)?;
        refuse_busy_disk(&snapshot, device)?;
        require_partition(&snapshot, device, args.number)?;
        let call = plan_delete(&device.path, args.number)?;
        self.run_plan(&call)?;
        Ok(format!(
            "deleted partition {} on {}",
            args.number, device.path
        ))
    }

    pub fn format_partition(&self, request: &VolumeFormatRequest) -> Result<String, WireError> {
        if !self.config.format {
            return Err(disabled(
                "volume_format_disabled",
                "`[volumes] format` is off",
            ));
        }
        let snapshot = self.snapshot();
        let known = snapshot
            .devices
            .iter()
            .find(|device| device.path == request.partition)
            .ok_or_else(|| not_found(format!("`{}` is not a known device", request.partition)))?;
        if known.kind != VolumeKind::Part {
            return Err(invalid(format!(
                "`{}` is a {} and not a partition",
                request.partition,
                known.kind.as_str()
            )));
        }
        let parent = known
            .parent
            .as_deref()
            .ok_or_else(|| invalid(format!("`{}` has no parent disk", request.partition)))?;
        let disk = snapshot
            .devices
            .iter()
            .find(|device| device.kind == VolumeKind::Disk && device.name == parent)
            .ok_or_else(|| {
                not_found(format!(
                    "the disk behind `{}` is unknown",
                    request.partition
                ))
            })?;
        allowed_disk(&snapshot, &self.config.disk_allow, &disk.path)?;
        if let Some(mount) = snapshot
            .mounts
            .iter()
            .find(|mount| mount.source == request.partition)
        {
            return Err(busy(format!(
                "`{}` is mounted on `{}`; unmount it before formatting",
                request.partition, mount.mountpoint
            )));
        }
        refuse_active_swap(&snapshot, &[request.partition.as_str(), disk.path.as_str()])?;
        let call = plan_format(
            &request.partition,
            &request.fs_type,
            request.label.as_deref(),
        )?;
        self.run_plan(&call)?;
        Ok(format!(
            "formatted {} as {}",
            request.partition, request.fs_type
        ))
    }

    fn run_plan(&self, call: &PlannedCall) -> Result<(), WireError> {
        let output = match call.stdin.as_deref() {
            Some(script) => self.runner.run_with_stdin(call.program, &call.argv, script),
            None => self.runner.run(call.program, &call.argv),
        };
        match output {
            Err(detail) => Err(WireError::new(
                ErrorKind::Unsupported,
                "partition_unavailable",
                format!(
                    "`{}` is not installed or could not be executed: {detail}",
                    call.program
                ),
            )),
            Ok(output) if output.ok => Ok(()),
            Ok(output) => Err(classify_failure(call.program, &output)),
        }
    }
}

fn require_partition(
    snapshot: &VolumeSnapshot,
    disk: &VolumeDevice,
    number: u32,
) -> Result<(), WireError> {
    let exists = snapshot.devices.iter().any(|device| {
        device.kind == VolumeKind::Part
            && device.parent.as_deref() == Some(disk.name.as_str())
            && partition_number(&device.name) == Some(number)
    });
    if !exists {
        return Err(not_found(format!(
            "partition {number} does not exist on `{}`",
            disk.path
        )));
    }
    Ok(())
}

fn classify_failure(program: &str, output: &MountOutput) -> WireError {
    let lowercase = output.stderr.to_lowercase();
    let (kind, code) = if lowercase.contains("busy") || lowercase.contains("in use") {
        (ErrorKind::Conflict, "disk_busy")
    } else if lowercase.contains("permission denied")
        || lowercase.contains("operation not permitted")
        || lowercase.contains("must be superuser")
        || lowercase.contains("only root")
    {
        (ErrorKind::Forbidden, "partition_permission")
    } else if lowercase.contains("does not exist")
        || lowercase.contains("no such file")
        || lowercase.contains("not found")
    {
        (ErrorKind::NotFound, "partition_not_found")
    } else if lowercase.contains("wrong fs type")
        || lowercase.contains("unknown filesystem")
        || lowercase.contains("bad option")
        || lowercase.contains("invalid")
        || lowercase.contains("syntax")
    {
        (ErrorKind::Invalid, "partition_refused")
    } else {
        (ErrorKind::Internal, "partition_failed")
    };
    let stderr = output
        .stderr
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("; ");
    WireError::new(
        kind,
        code,
        if stderr.is_empty() {
            format!("{program} refused the change")
        } else {
            format!("{program} refused the change: {stderr}")
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use anth_wire::{VolumeKind, VolumeMount, VolumeSnapshot};

    fn disk() -> VolumeDevice {
        VolumeDevice {
            name: "sda".to_string(),
            path: "/dev/sda".to_string(),
            kind: VolumeKind::Disk,
            parent: None,
            size_bytes: 1000,
            fstype: None,
            label: None,
            uuid: None,
            mountpoints: Vec::new(),
            read_only: false,
            removable: false,
            model: None,
            transport: None,
        }
    }

    fn partition(name: &str) -> VolumeDevice {
        VolumeDevice {
            name: name.to_string(),
            path: format!("/dev/{name}"),
            kind: VolumeKind::Part,
            parent: Some("sda".to_string()),
            size_bytes: 100,
            fstype: None,
            label: None,
            uuid: None,
            mountpoints: Vec::new(),
            read_only: false,
            removable: false,
            model: None,
            transport: None,
        }
    }

    fn mount(mountpoint: &str, source: &str) -> VolumeMount {
        VolumeMount {
            mountpoint: mountpoint.to_string(),
            source: source.to_string(),
            fstype: "ext4".to_string(),
            options: "rw".to_string(),
            read_only: false,
            virtual_fs: false,
            usage: None,
        }
    }

    fn snapshot(
        devices: Vec<VolumeDevice>,
        mounts: Vec<VolumeMount>,
        swaps: Vec<&str>,
    ) -> VolumeSnapshot {
        VolumeSnapshot {
            devices,
            mounts,
            swap_devices: swaps.into_iter().map(str::to_string).collect(),
            control_enabled: true,
            partition_enabled: true,
            format_enabled: true,
            mount_root: "/mnt".to_string(),
            disk_allow: vec!["/dev/sda".to_string()],
            warning: None,
            timestamp_ms: 1,
        }
    }

    #[test]
    fn a_disk_must_be_whole_and_allowlisted() {
        let snapshot = snapshot(vec![disk(), partition("sda1")], vec![], vec![]);
        assert!(allowed_disk(&snapshot, &["/dev/sda".to_string()], "/dev/sda").is_ok());
        assert_eq!(
            allowed_disk(&snapshot, &[], "/dev/sda")
                .expect_err("empty allowlist")
                .code,
            "protected_disk"
        );
        assert_eq!(
            allowed_disk(&snapshot, &["/dev/sdb".to_string()], "/dev/sda")
                .expect_err("not allowed")
                .code,
            "protected_disk"
        );
        assert_eq!(
            allowed_disk(&snapshot, &["/dev/sda".to_string()], "/dev/sda1")
                .expect_err("partition")
                .code,
            "invalid_partition"
        );
        assert!(validate_device_path("../sda").is_err());
        assert!(validate_device_path("/dev/../sda").is_err());
    }

    #[test]
    fn protected_busy_swap_and_whole_disk_filesystems_are_refused() {
        let root = snapshot(vec![disk()], vec![mount("/", "/dev/sda1")], vec![]);
        assert_eq!(
            refuse_protected_disk(&root, &root.devices[0], &[])
                .expect_err("root")
                .code,
            "protected_disk"
        );
        assert_eq!(
            refuse_busy_disk(&root, &root.devices[0])
                .expect_err("busy")
                .code,
            "disk_busy"
        );

        let swap = snapshot(vec![disk()], vec![], vec!["/dev/sda2"]);
        assert_eq!(
            refuse_busy_disk(&swap, &swap.devices[0])
                .expect_err("swap")
                .code,
            "disk_busy"
        );
        assert_eq!(
            refuse_active_swap(&swap, &["/dev/sda2"])
                .expect_err("swap target")
                .code,
            "disk_busy"
        );

        let mut whole = disk();
        whole.fstype = Some("vfat".to_string());
        assert_eq!(
            refuse_whole_disk_filesystem(&whole)
                .expect_err("whole disk")
                .code,
            "disk_busy"
        );
    }

    #[test]
    fn sfdisk_plans_use_stdin_and_never_a_shell_string() {
        let call = plan_edit(
            "/dev/sda",
            &TableEdit::Create {
                kind: PartitionKind::Linux,
                start_bytes: 1024,
                size_bytes: 4096,
            },
            true,
        )
        .expect("create");
        assert_eq!(call.program, "sfdisk");
        assert_eq!(call.argv, vec!["--no-reread", "--append", "/dev/sda"]);
        assert_eq!(call.stdin.as_deref(), Some("start=2,size=8,type=linux"));
        assert!(
            plan_edit(
                "/dev/sda",
                &TableEdit::Create {
                    kind: PartitionKind::Linux,
                    start_bytes: 0,
                    size_bytes: 513,
                },
                false,
            )
            .is_err()
        );

        let resize = plan_resize("/dev/sda", 2, 8192).expect("resize");
        assert_eq!(resize.argv, vec!["--no-reread", "-N", "2", "/dev/sda"]);
        assert_eq!(resize.stdin.as_deref(), Some("size=16"));

        let delete = plan_delete("/dev/sda", 2).expect("delete");
        assert_eq!(
            delete.argv,
            vec!["--no-reread", "--delete", "/dev/sda", "2"]
        );
        assert!(delete.stdin.is_none());
    }

    #[test]
    fn format_types_map_to_the_right_program_and_label_flag() {
        let ext4 = plan_format("/dev/sda1", "ext4", Some("root")).expect("ext4");
        assert_eq!(ext4.program, "mkfs.ext4");
        assert_eq!(ext4.argv, vec!["-L", "root", "/dev/sda1"]);
        let vfat = plan_format("/dev/sda1", "vfat", Some("BOOT")).expect("vfat");
        assert_eq!(vfat.program, "mkfs.vfat");
        assert_eq!(vfat.argv, vec!["-n", "BOOT", "/dev/sda1"]);
        let swap = plan_format("/dev/sda1", "swap", None).expect("swap");
        assert_eq!(swap.program, "mkswap");
        assert_eq!(swap.argv, vec!["/dev/sda1"]);
        assert!(plan_format("/dev/sda1", "ntfs", None).is_err());
        assert!(plan_format("/dev/sda1", "vfat", Some("TOO-LONG-LABEL")).is_err());
    }

    #[test]
    fn partition_numbers_follow_the_device_name() {
        assert_eq!(partition_number("sda3"), Some(3));
        assert_eq!(partition_number("nvme0n1p14"), Some(14));
        assert_eq!(partition_number("sda"), None);
    }
}
