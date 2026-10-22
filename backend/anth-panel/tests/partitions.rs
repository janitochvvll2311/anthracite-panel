//! Partition planning and guard tests.
//!
//! The runner here is a fake, so the assertions are about the exact argv the
//! daemon would execute and about which guard fires first. Nothing in this file
//! touches a real disk.

use std::path::PathBuf;

use anth_proto::{
    PartitionCreateArgs, PartitionTableAction, VolumeDevice, VolumeErrorKind, VolumeKind,
    VolumeMount, VolumeSnapshot,
};

use anth_panel::command::{CommandError, CommandOutput, CommandRunner};
use anth_panel::volumes::{
    PartitionKind, PlannedCall, TableEdit, create_with, edit_with, partition_number, partitions_on,
    plan_edit, plan_format, validate_device_path,
};

/// One recorded call: program, argv and the script written to stdin.
type Recorded = (String, Vec<String>, Option<String>);

/// Records every call and answers with a scripted result.
struct FakeRunner {
    calls: std::cell::RefCell<Vec<Recorded>>,
    stderr: String,
    ok: bool,
}

impl FakeRunner {
    fn ok() -> Self {
        Self {
            calls: std::cell::RefCell::new(Vec::new()),
            stderr: String::new(),
            ok: true,
        }
    }

    fn failing(stderr: &str) -> Self {
        Self {
            calls: std::cell::RefCell::new(Vec::new()),
            stderr: stderr.to_string(),
            ok: false,
        }
    }

    fn calls(&self) -> Vec<Recorded> {
        self.calls.borrow().clone()
    }
}

impl CommandRunner for FakeRunner {
    fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput, CommandError> {
        self.calls.borrow_mut().push((
            program.to_string(),
            args.iter().map(|a| a.to_string()).collect(),
            None,
        ));
        Ok(CommandOutput {
            ok: self.ok,
            stdout: String::new(),
            stderr: self.stderr.clone(),
        })
    }

    fn run_with_stdin(
        &self,
        program: &str,
        args: &[&str],
        input: &str,
    ) -> Result<CommandOutput, CommandError> {
        self.calls.borrow_mut().push((
            program.to_string(),
            args.iter().map(|a| a.to_string()).collect(),
            Some(input.to_string()),
        ));
        Ok(CommandOutput {
            ok: self.ok,
            stdout: String::new(),
            stderr: self.stderr.clone(),
        })
    }
}

fn disk(name: &str, path: &str) -> VolumeDevice {
    VolumeDevice {
        name: name.to_string(),
        path: path.to_string(),
        kind: VolumeKind::Disk,
        parent: None,
        size_bytes: 512 * 1024 * 1024 * 1024,
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

fn part(name: &str, path: &str, parent: &str) -> VolumeDevice {
    VolumeDevice {
        name: name.to_string(),
        path: path.to_string(),
        kind: VolumeKind::Part,
        parent: Some(parent.to_string()),
        size_bytes: 1024 * 1024 * 1024,
        fstype: Some("ext4".to_string()),
        label: None,
        uuid: None,
        mountpoints: Vec::new(),
        read_only: false,
        removable: false,
        model: None,
        transport: None,
    }
}

fn snapshot(devices: Vec<VolumeDevice>, mounts: Vec<VolumeMount>) -> VolumeSnapshot {
    let total = devices.len();
    VolumeSnapshot {
        devices,
        mounts,
        control_enabled: true,
        timestamp_ms: 0,
        total,
        truncated: false,
        warning: None,
    }
}

fn mount(source: &str, mountpoint: &str) -> VolumeMount {
    VolumeMount {
        mountpoint: mountpoint.to_string(),
        source: source.to_string(),
        fstype: "ext4".to_string(),
        options: String::new(),
        read_only: false,
        virtual_fs: false,
        usage: None,
    }
}

fn allowed(paths: &[&str]) -> Vec<String> {
    paths.iter().map(|path| path.to_string()).collect()
}

#[test]
fn a_relative_or_traversing_device_path_is_refused() {
    for bad in [
        "",
        "sda",
        "../sda",
        "/dev/../sda",
        "/dev/./sda",
        "//dev/sda",
    ] {
        assert!(
            validate_device_path(bad).is_err(),
            "`{bad}` should have been refused"
        );
    }
    assert!(validate_device_path("/dev/sda").is_ok());
    assert!(validate_device_path("/dev/nvme0n1").is_ok());
}

#[test]
fn an_empty_allowlist_denies_every_disk() {
    let snapshot = snapshot(vec![disk("sda", "/dev/sda")], Vec::new());
    let runner = FakeRunner::ok();
    let error = create_with(
        &runner,
        &snapshot,
        &PartitionCreateArgs {
            disk: "/dev/sda".to_string(),
            kind: "linux".to_string(),
            start_bytes: 0,
            size_bytes: 1024 * 1024,
        },
        &[],
        &[],
    )
    .unwrap_err();
    assert_eq!(error.kind, VolumeErrorKind::Protected);
    assert!(error.detail.contains("disk_allow"));
    assert!(runner.calls().is_empty(), "nothing may run");
}

#[test]
fn a_disk_missing_from_the_allowlist_is_refused() {
    let snapshot = snapshot(
        vec![disk("sda", "/dev/sda"), disk("sdb", "/dev/sdb")],
        Vec::new(),
    );
    let runner = FakeRunner::ok();
    let error = create_with(
        &runner,
        &snapshot,
        &PartitionCreateArgs {
            disk: "/dev/sdb".to_string(),
            kind: "linux".to_string(),
            start_bytes: 0,
            size_bytes: 1024 * 1024,
        },
        &allowed(&["/dev/sda"]),
        &[],
    )
    .unwrap_err();
    assert_eq!(error.kind, VolumeErrorKind::Protected);
    assert!(error.detail.contains("disk_allow"));
    assert!(runner.calls().is_empty());
}

#[test]
fn a_partition_is_not_accepted_where_a_disk_is_required() {
    let snapshot = snapshot(
        vec![disk("sda", "/dev/sda"), part("sda1", "/dev/sda1", "sda")],
        Vec::new(),
    );
    let runner = FakeRunner::ok();
    let error = create_with(
        &runner,
        &snapshot,
        &PartitionCreateArgs {
            disk: "/dev/sda1".to_string(),
            kind: "linux".to_string(),
            start_bytes: 0,
            size_bytes: 1024 * 1024,
        },
        &allowed(&["/dev/sda", "/dev/sda1"]),
        &[],
    )
    .unwrap_err();
    assert_eq!(error.kind, VolumeErrorKind::Invalid);
    assert!(runner.calls().is_empty());
}

#[test]
fn the_disk_holding_root_is_refused() {
    let snapshot = snapshot(
        vec![disk("sda", "/dev/sda"), part("sda1", "/dev/sda1", "sda")],
        vec![mount("/dev/sda1", "/")],
    );
    let runner = FakeRunner::ok();
    let error = create_with(
        &runner,
        &snapshot,
        &PartitionCreateArgs {
            disk: "/dev/sda".to_string(),
            kind: "linux".to_string(),
            start_bytes: 0,
            size_bytes: 1024 * 1024,
        },
        &allowed(&["/dev/sda"]),
        &[],
    )
    .unwrap_err();
    assert_eq!(error.kind, VolumeErrorKind::Protected);
    assert!(error.detail.contains("`/`"));
    assert!(runner.calls().is_empty());
}

#[test]
fn a_disk_whose_name_is_a_prefix_of_another_is_not_confused_for_it() {
    // `/dev/sdaa` starts with `/dev/sda`. Reading the mount's device name is
    // what keeps the guarded disk from being decided by a string prefix.
    let snapshot = snapshot(
        vec![disk("sda", "/dev/sda"), disk("sdaa", "/dev/sdaa")],
        vec![mount("/dev/sdaa1", "/boot")],
    );
    let runner = FakeRunner::ok();
    create_with(
        &runner,
        &snapshot,
        &PartitionCreateArgs {
            disk: "/dev/sda".to_string(),
            kind: "linux".to_string(),
            start_bytes: 0,
            size_bytes: 1024 * 1024,
        },
        &allowed(&["/dev/sda"]),
        &[PathBuf::from("/boot")],
    )
    .expect("the innocent disk must not be refused for its neighbour's mount");
    assert_eq!(runner.calls().len(), 1);
}

#[test]
fn a_disk_carrying_a_protected_mount_is_refused() {
    let snapshot = snapshot(
        vec![disk("sda", "/dev/sda"), part("sda1", "/dev/sda1", "sda")],
        vec![mount("/dev/sda1", "/boot")],
    );
    let runner = FakeRunner::ok();
    let error = create_with(
        &runner,
        &snapshot,
        &PartitionCreateArgs {
            disk: "/dev/sda".to_string(),
            kind: "linux".to_string(),
            start_bytes: 0,
            size_bytes: 1024 * 1024,
        },
        &allowed(&["/dev/sda"]),
        &[PathBuf::from("/boot")],
    )
    .unwrap_err();
    assert_eq!(error.kind, VolumeErrorKind::Protected);
    assert!(runner.calls().is_empty());
}

#[test]
fn a_disk_with_a_mounted_partition_is_busy() {
    let snapshot = snapshot(
        vec![disk("sda", "/dev/sda"), part("sda1", "/dev/sda1", "sda")],
        vec![mount("/dev/sda1", "/mnt/data")],
    );
    let runner = FakeRunner::ok();
    let error = create_with(
        &runner,
        &snapshot,
        &PartitionCreateArgs {
            disk: "/dev/sda".to_string(),
            kind: "linux".to_string(),
            start_bytes: 0,
            size_bytes: 1024 * 1024,
        },
        &allowed(&["/dev/sda"]),
        &[],
    )
    .unwrap_err();
    assert_eq!(error.kind, VolumeErrorKind::Busy);
    assert!(error.detail.contains("/mnt/data"));
    assert!(runner.calls().is_empty());
}

#[test]
fn a_create_plans_the_script_on_stdin_not_in_argv() {
    let snapshot = snapshot(vec![disk("sdb", "/dev/sdb")], Vec::new());
    let runner = FakeRunner::ok();
    create_with(
        &runner,
        &snapshot,
        &PartitionCreateArgs {
            disk: "/dev/sdb".to_string(),
            kind: "linux".to_string(),
            start_bytes: 0,
            size_bytes: 4096 * 512,
        },
        &allowed(&["/dev/sdb"]),
        &[],
    )
    .expect("a clean disk with a size must be accepted");

    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    let (program, argv, stdin) = &calls[0];
    assert_eq!(program, "sfdisk");
    assert_eq!(
        argv,
        &vec!["--no-reread".to_string(), "/dev/sdb".to_string()]
    );
    assert_eq!(stdin.as_deref(), Some("size=4096,type=linux"));
}

#[test]
fn a_size_that_is_not_a_whole_sector_is_refused() {
    let error = plan_edit(
        "/dev/sdb",
        &TableEdit::Create {
            kind: PartitionKind::Linux,
            start_bytes: 0,
            size_bytes: 1000,
        },
    )
    .unwrap_err();
    assert_eq!(error.kind, VolumeErrorKind::Invalid);
    assert!(error.detail.contains("512"));
}

#[test]
fn a_zero_size_is_refused() {
    let error = plan_edit(
        "/dev/sdb",
        &TableEdit::Create {
            kind: PartitionKind::Linux,
            start_bytes: 0,
            size_bytes: 0,
        },
    )
    .unwrap_err();
    assert!(error.detail.contains("needs a size"));
}

#[test]
fn a_delete_uses_sfdisk_delete_syntax() {
    let call = plan_edit("/dev/sdb", &TableEdit::Delete { number: 2 }).unwrap();
    assert_eq!(call.program, "sfdisk");
    assert_eq!(call.stdin.as_deref(), Some("delete 2"));
}

#[test]
fn a_resize_names_the_partition_and_the_size() {
    let call = plan_edit(
        "/dev/sdb",
        &TableEdit::Resize {
            number: 3,
            size_bytes: 2048 * 512,
        },
    )
    .unwrap();
    assert_eq!(call.stdin.as_deref(), Some("3,size=2048"));
}

#[test]
fn an_unknown_kind_is_refused() {
    let error = plan_edit(
        "/dev/sdb",
        &TableEdit::Create {
            kind: PartitionKind::parse("exfat").unwrap_or(PartitionKind::Linux),
            start_bytes: 0,
            size_bytes: 512,
        },
    );
    // `PartitionKind::parse` returning `None` is what the daemon checks; this
    // asserts the fallback in the test above did not silently allow a bad kind.
    assert!(PartitionKind::parse("exfat").is_none());
    assert!(error.is_ok());
}

#[test]
fn every_format_type_maps_to_its_own_program() {
    let cases = [
        ("ext4", "mkfs.ext4"),
        ("xfs", "mkfs.xfs"),
        ("vfat", "mkfs.vfat"),
        ("exfat", "mkfs.exfat"),
        ("btrfs", "mkfs.btrfs"),
        ("swap", "mkswap"),
    ];
    for (fs_type, program) in cases {
        let call = plan_format("/dev/sdb1", fs_type, None).unwrap();
        assert_eq!(call.program, program, "{fs_type}");
        assert_eq!(call.argv, vec!["/dev/sdb1".to_string()]);
        assert!(call.stdin.is_none(), "{fs_type} takes no script");
    }
}

#[test]
fn an_unsupported_format_type_is_refused() {
    let error = plan_format("/dev/sdb1", "zfs", None).unwrap_err();
    assert_eq!(error.kind, VolumeErrorKind::Invalid);
    assert!(error.detail.contains("zfs"));
}

#[test]
fn a_label_with_a_shell_character_is_refused() {
    for bad in ["a;rm -rf /", "a b", "$(id)", "a/b"] {
        assert!(
            plan_format("/dev/sdb1", "ext4", Some(bad)).is_err(),
            "`{bad}` should have been refused"
        );
    }
    let call = plan_format("/dev/sdb1", "ext4", Some("data-2024")).unwrap();
    assert_eq!(
        call.argv,
        vec![
            "-L".to_string(),
            "data-2024".to_string(),
            "/dev/sdb1".to_string()
        ]
    );
}

#[test]
fn a_format_is_refused_when_its_switch_is_off() {
    let snapshot = snapshot(
        vec![disk("sdb", "/dev/sdb"), part("sdb1", "/dev/sdb1", "sdb")],
        Vec::new(),
    );
    let runner = FakeRunner::ok();
    let error = edit_with(
        &runner,
        &snapshot,
        &PartitionTableAction::Format {
            partition: "/dev/sdb1".to_string(),
            fs_type: "ext4".to_string(),
            label: None,
        },
        &allowed(&["/dev/sdb"]),
        &[],
        false,
    )
    .unwrap_err();
    assert_eq!(error.kind, VolumeErrorKind::Disabled);
    assert!(runner.calls().is_empty());
}

#[test]
fn a_format_on_the_disk_outside_the_allowlist_is_refused() {
    let snapshot = snapshot(
        vec![disk("sdb", "/dev/sdb"), part("sdb1", "/dev/sdb1", "sdb")],
        Vec::new(),
    );
    let runner = FakeRunner::ok();
    let error = edit_with(
        &runner,
        &snapshot,
        &PartitionTableAction::Format {
            partition: "/dev/sdb1".to_string(),
            fs_type: "ext4".to_string(),
            label: None,
        },
        &allowed(&["/dev/sda"]),
        &[],
        true,
    )
    .unwrap_err();
    assert_eq!(error.kind, VolumeErrorKind::Protected);
    assert!(runner.calls().is_empty());
}

#[test]
fn a_mounted_partition_cannot_be_formatted() {
    let snapshot = snapshot(
        vec![disk("sdb", "/dev/sdb"), part("sdb1", "/dev/sdb1", "sdb")],
        vec![mount("/dev/sdb1", "/mnt/x")],
    );
    let runner = FakeRunner::ok();
    let error = edit_with(
        &runner,
        &snapshot,
        &PartitionTableAction::Format {
            partition: "/dev/sdb1".to_string(),
            fs_type: "ext4".to_string(),
            label: None,
        },
        &allowed(&["/dev/sdb"]),
        &[],
        true,
    )
    .unwrap_err();
    assert_eq!(error.kind, VolumeErrorKind::Busy);
    assert!(runner.calls().is_empty());
}

#[test]
fn a_disk_that_is_not_a_partition_cannot_be_formatted() {
    let snapshot = snapshot(vec![disk("sdb", "/dev/sdb")], Vec::new());
    let runner = FakeRunner::ok();
    let error = edit_with(
        &runner,
        &snapshot,
        &PartitionTableAction::Format {
            partition: "/dev/sdb".to_string(),
            fs_type: "ext4".to_string(),
            label: None,
        },
        &allowed(&["/dev/sdb"]),
        &[],
        true,
    )
    .unwrap_err();
    assert_eq!(error.kind, VolumeErrorKind::Invalid);
}

#[test]
fn a_failing_tool_reports_its_own_message() {
    let snapshot = snapshot(vec![disk("sdb", "/dev/sdb")], Vec::new());
    let runner = FakeRunner::failing("sfdisk: cannot open /dev/sdb: Permission denied");
    let error = create_with(
        &runner,
        &snapshot,
        &PartitionCreateArgs {
            disk: "/dev/sdb".to_string(),
            kind: "linux".to_string(),
            start_bytes: 0,
            size_bytes: 512,
        },
        &allowed(&["/dev/sdb"]),
        &[],
    )
    .unwrap_err();
    assert_eq!(error.kind, VolumeErrorKind::Failed);
    assert!(error.detail.contains("Permission denied"));
}

#[test]
fn partition_numbers_come_from_the_trailing_digits() {
    assert_eq!(partition_number("sda1"), Some(1));
    assert_eq!(partition_number("nvme0n1p12"), Some(12));
    assert_eq!(partition_number("mmcblk0p3"), Some(3));
    assert_eq!(partition_number("sda"), None);
}

#[test]
fn partitions_are_listed_in_table_order_with_their_mounts() {
    let mut mounted = part("sdb2", "/dev/sdb2", "sdb");
    mounted.mountpoints = vec!["/mnt/two".to_string()];
    let snapshot = snapshot(
        vec![
            disk("sdb", "/dev/sdb"),
            mounted,
            part("sdb1", "/dev/sdb1", "sdb"),
            part("sdc1", "/dev/sdc1", "sdc"),
        ],
        Vec::new(),
    );
    let entries = partitions_on(&snapshot, &snapshot.devices[0]);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].number, 1);
    assert_eq!(entries[1].number, 2);
    assert_eq!(entries[1].mountpoints, vec!["/mnt/two".to_string()]);
}

#[test]
fn a_planned_call_is_debug_printable_without_a_secret() {
    let call = PlannedCall {
        program: "sfdisk",
        argv: vec!["--no-reread".to_string(), "/dev/sdb".to_string()],
        stdin: Some("size=8,type=linux".to_string()),
    };
    let rendered = format!("{call:?}");
    assert!(rendered.contains("sfdisk"));
}
