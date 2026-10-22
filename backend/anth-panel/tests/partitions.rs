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
    plan_delete, plan_edit, plan_format, plan_resize, validate_device_path,
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
        swap_devices: Vec::new(),
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
        &vec!["--no-reread".to_string(), "/dev/sdb".to_string()],
        "this fixture disk has no partitions, so --append would be wrong here"
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
        true,
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
        true,
    )
    .unwrap_err();
    assert!(error.detail.contains("needs a size"));
}

#[test]
fn a_delete_is_a_verb_in_argv_not_a_script_line() {
    let call = plan_delete("/dev/sdb", 2).unwrap();
    assert_eq!(call.program, "sfdisk");
    assert_eq!(
        call.argv,
        vec![
            "--no-reread".to_string(),
            "--delete".to_string(),
            "/dev/sdb".to_string(),
            "2".to_string()
        ]
    );
    assert!(
        call.stdin.is_none(),
        "`delete 2` is not input-format syntax and sfdisk rejects it"
    );
}

#[test]
fn a_resize_selects_the_partition_with_n() {
    let call = plan_resize("/dev/sdb", 3, 2048 * 512).unwrap();
    assert_eq!(
        call.argv,
        vec![
            "--no-reread".to_string(),
            "-N".to_string(),
            "3".to_string(),
            "/dev/sdb".to_string()
        ]
    );
    assert_eq!(
        call.stdin.as_deref(),
        Some("size=2048"),
        "a leading `<n>,` is not input-format syntax"
    );
}

#[test]
fn an_unknown_kind_is_refused() {
    assert!(PartitionKind::parse("exfat").is_none());
    assert!(PartitionKind::parse("linux").is_some());
}

#[test]
fn a_vfat_label_uses_n_and_is_capped_at_eleven() {
    let call = plan_format("/dev/sdb1", "vfat", Some("DATA")).unwrap();
    assert_eq!(
        call.argv,
        vec![
            "-n".to_string(),
            "DATA".to_string(),
            "/dev/sdb1".to_string()
        ],
        "mkfs.fat rejects -L"
    );
    let error = plan_format("/dev/sdb1", "vfat", Some("TWELVE_CHARS")).unwrap_err();
    assert_eq!(error.kind, VolumeErrorKind::Invalid);
    assert!(error.detail.contains("11"));
    // ext4 keeps the 32-character limit and `-L`.
    let ext4 = plan_format("/dev/sdb1", "ext4", Some("a_rather_long_label_here_ok")).unwrap();
    assert_eq!(ext4.argv[0], "-L");
}

#[test]
fn a_resize_of_partition_zero_is_refused() {
    assert!(plan_resize("/dev/sdb", 0, 512).is_err());
    assert!(plan_delete("/dev/sdb", 0).is_err());
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

// ---------------------------------------------------------------------------
// Real-binary tests.
//
// Everything above asserts the string the daemon *would* send; a fake accepts
// any stdin, so it cannot tell working `sfdisk` syntax from broken. These run
// the planned call against a real `sfdisk` and a throwaway image, and skip when
// the tool is absent so a minimal CI image still passes.
// ---------------------------------------------------------------------------

/// PATH plus the sbin directories, where util-linux lives on Debian and Alpine.
fn path_with_sbin() -> String {
    let current = std::env::var("PATH").unwrap_or_default();
    format!("{current}:/sbin:/usr/sbin")
}

fn have(program: &str) -> bool {
    if program.contains('/') {
        return std::path::Path::new(program).exists();
    }
    path_with_sbin()
        .split(':')
        .any(|dir| std::path::Path::new(dir).join(program).exists())
}

/// Runs one planned call for real. The program is resolved through the first
/// PATH entry that holds it, since `PlannedCall.program` is a bare name.
fn run_real(call: &PlannedCall, path: &str) -> std::process::Output {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut command = Command::new(call.program);
    command
        .args(&call.argv)
        .env("PATH", path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command.stdin(if call.stdin.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    });

    let mut child = command.spawn().expect("the tool must start");
    if let Some(script) = &call.stdin {
        child
            .stdin
            .as_mut()
            .expect("stdin was piped")
            .write_all(script.as_bytes())
            .expect("the script must reach the tool");
    }
    child.wait_with_output().expect("the tool must finish")
}

/// A sparse image with three partitions, the state a create must preserve.
fn three_partition_image(dir: &std::path::Path, path: &str) -> std::path::PathBuf {
    let image = dir.join("disk.img");
    let file = std::fs::File::create(&image).unwrap();
    file.set_len(64 * 1024 * 1024).unwrap();
    let script = "size=8192,type=linux\nsize=8192,type=linux\nsize=8192,type=linux\n";
    let mut child = std::process::Command::new("sfdisk")
        .arg(&image)
        .env("PATH", path)
        .stdin(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    use std::io::Write;
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(script.as_bytes())
        .unwrap();
    child.wait().unwrap();
    image
}

fn partition_count(image: &std::path::Path, path: &str) -> usize {
    let output = std::process::Command::new("sfdisk")
        .arg("-l")
        .arg(image)
        .env("PATH", path)
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| line.starts_with(&image.display().to_string()))
        .count()
}

#[test]
fn a_real_sfdisk_appends_instead_of_replacing_the_table() {
    let path = path_with_sbin();
    if !have("sfdisk") {
        eprintln!("skipping: sfdisk is not installed");
        return;
    }
    let dir = std::env::temp_dir().join(format!("anth-part-append-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let image = three_partition_image(&dir, &path);
    assert_eq!(
        partition_count(&image, &path),
        3,
        "the fixture must start at 3"
    );

    let call = plan_edit(
        &image.display().to_string(),
        &TableEdit::Create {
            kind: PartitionKind::Linux,
            start_bytes: 0,
            size_bytes: 1024 * 1024,
        },
        true,
    )
    .unwrap();
    let output = run_real(&call, &path);
    assert!(
        output.status.success(),
        "sfdisk failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert_eq!(
        partition_count(&image, &path),
        4,
        "a create must add a partition, not rewrite the table"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_real_sfdisk_accepts_the_resize_script() {
    let path = path_with_sbin();
    if !have("sfdisk") {
        eprintln!("skipping: sfdisk is not installed");
        return;
    }
    let dir = std::env::temp_dir().join(format!("anth-part-resize-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let image = three_partition_image(&dir, &path);

    let call = plan_resize(&image.display().to_string(), 3, 4096 * 512).unwrap();
    let output = run_real(&call, &path);
    assert!(
        output.status.success(),
        "sfdisk rejected the resize script: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let listing = std::process::Command::new("sfdisk")
        .arg("-l")
        .arg(&image)
        .env("PATH", &path)
        .output()
        .unwrap();
    let listing = String::from_utf8_lossy(&listing.stdout).into_owned();
    let third = listing
        .lines()
        .find(|line| line.contains("img3"))
        .expect("partition 3 must survive");
    assert!(
        third.contains("4096"),
        "partition 3 must be 4096 sectors: {third}"
    );
    assert_eq!(
        partition_count(&image, &path),
        3,
        "a resize must not drop rows"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_real_sfdisk_accepts_the_delete_verb() {
    let path = path_with_sbin();
    if !have("sfdisk") {
        eprintln!("skipping: sfdisk is not installed");
        return;
    }
    let dir = std::env::temp_dir().join(format!("anth-part-delete-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let image = three_partition_image(&dir, &path);

    let call = plan_delete(&image.display().to_string(), 2).unwrap();
    let output = run_real(&call, &path);
    assert!(
        output.status.success(),
        "sfdisk rejected `delete`: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        partition_count(&image, &path),
        2,
        "only partition 2 must be removed"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_real_mkfs_vfat_accepts_the_label_flag_the_planner_emits() {
    let path = path_with_sbin();
    if !have("mkfs.vfat") {
        eprintln!("skipping: mkfs.vfat is not installed");
        return;
    }
    let dir = std::env::temp_dir().join(format!("anth-part-vfat-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let image = dir.join("vfat.img");
    let file = std::fs::File::create(&image).unwrap();
    file.set_len(16 * 1024 * 1024).unwrap();

    let call = plan_format(&image.display().to_string(), "vfat", Some("DATA")).unwrap();
    let output = run_real(&call, &path);
    assert!(
        output.status.success(),
        "mkfs.vfat rejected the planned argv: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_truncated_snapshot_cannot_hide_a_busy_disk() {
    // `fit_snapshot` drops device and mount tails to fit the line cap. A guard
    // that read the fitted snapshot would then miss the mount that makes a
    // disk busy, so the mutation path must use the unfitted collector. This
    // asserts the fitting itself is lossy, which is why the distinction
    // matters at all.
    let mut snapshot = snapshot(vec![disk("sdb", "/dev/sdb")], Vec::new());
    for index in 0..6000 {
        snapshot.mounts.push(mount(
            &format!("/dev/sdb{}", (index % 100) + 1),
            &format!("/mnt/filler-{index}-{}", "x".repeat(160)),
        ));
    }
    let fitted = anth_panel::volumes::fit_snapshot_for_test(snapshot.clone());
    assert!(
        fitted.mounts.len() < snapshot.mounts.len(),
        "the fixture must be large enough to be shortened, or this proves nothing"
    );
    assert!(
        fitted.warning.is_some(),
        "a shortened snapshot reports the truncation"
    );
    // `refuse_busy_disk` still finds the mount in the unfitted snapshot, which
    // is the one the mutation path now collects.
    let disk = snapshot
        .devices
        .iter()
        .find(|device| device.path == "/dev/sdb")
        .unwrap();
    let error = anth_panel::volumes::refuse_busy_disk(&snapshot, disk).unwrap_err();
    assert_eq!(error.kind, VolumeErrorKind::Busy);
}

#[test]
fn a_blank_disk_gets_a_fresh_table_instead_of_append() {
    // `--append` needs an existing disklabel and fails on a blank disk, which
    // is the ordinary first use of the feature. The planner must drop the flag
    // when the snapshot shows no partitions on the disk.
    let call = plan_edit(
        "/dev/sdb",
        &TableEdit::Create {
            kind: PartitionKind::Linux,
            start_bytes: 0,
            size_bytes: 1024 * 1024,
        },
        false,
    )
    .unwrap();
    assert_eq!(
        call.argv,
        vec!["--no-reread".to_string(), "/dev/sdb".to_string()],
        "a disk with no partitions must not pass --append"
    );
    assert_eq!(call.stdin.as_deref(), Some("size=2048,type=linux"));
}

#[test]
fn creating_the_first_partition_is_driven_by_the_snapshot() {
    // The same request must pick `--append` or not from the disk's children,
    // so this asserts the decision, not just the planner.
    let runner = FakeRunner::ok();
    let empty = snapshot(vec![disk("sdb", "/dev/sdb")], Vec::new());
    create_with(
        &runner,
        &empty,
        &PartitionCreateArgs {
            disk: "/dev/sdb".to_string(),
            kind: "linux".to_string(),
            start_bytes: 0,
            size_bytes: 1024 * 1024,
        },
        &allowed(&["/dev/sdb"]),
        &[],
    )
    .expect("a blank disk must accept its first partition");
    assert_eq!(
        runner.calls()[0].1,
        vec!["--no-reread".to_string(), "/dev/sdb".to_string()],
        "a blank disk must not get --append"
    );

    let runner = FakeRunner::ok();
    let populated = snapshot(
        vec![disk("sdb", "/dev/sdb"), part("sdb1", "/dev/sdb1", "sdb")],
        Vec::new(),
    );
    create_with(
        &runner,
        &populated,
        &PartitionCreateArgs {
            disk: "/dev/sdb".to_string(),
            kind: "linux".to_string(),
            start_bytes: 0,
            size_bytes: 1024 * 1024,
        },
        &allowed(&["/dev/sdb"]),
        &[],
    )
    .expect("a populated disk must accept another partition");
    assert_eq!(
        runner.calls()[0].1,
        vec![
            "--no-reread".to_string(),
            "--append".to_string(),
            "/dev/sdb".to_string()
        ],
        "a populated disk must get --append or lose its table"
    );
}

#[test]
fn a_real_sfdisk_creates_the_first_partition_on_a_blank_disk() {
    let path = path_with_sbin();
    if !have("sfdisk") {
        eprintln!("skipping: sfdisk is not installed");
        return;
    }
    let dir = std::env::temp_dir().join(format!("anth-part-blank-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let image = dir.join("blank.img");
    let file = std::fs::File::create(&image).unwrap();
    file.set_len(64 * 1024 * 1024).unwrap();
    assert_eq!(
        partition_count(&image, &path),
        0,
        "the image must start blank"
    );

    let call = plan_edit(
        &image.display().to_string(),
        &TableEdit::Create {
            kind: PartitionKind::Linux,
            start_bytes: 0,
            size_bytes: 1024 * 1024,
        },
        false,
    )
    .unwrap();
    let output = run_real(&call, &path);
    assert!(
        output.status.success(),
        "a blank disk must accept its first partition: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(partition_count(&image, &path), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_whole_disk_filesystem_is_refused() {
    // A USB stick formatted with mkfs.vfat has no partition table and no
    // children. Treating that as "blank" writes an MBR over the FAT boot
    // sector: the filesystem's metadata is gone while blkid still reports it.
    let mut usb = disk("sdb", "/dev/sdb");
    usb.fstype = Some("vfat".to_string());
    let snap = snapshot(vec![usb], Vec::new());
    let runner = FakeRunner::ok();
    let error = create_with(
        &runner,
        &snap,
        &PartitionCreateArgs {
            disk: "/dev/sdb".to_string(),
            kind: "linux".to_string(),
            start_bytes: 0,
            size_bytes: 1024 * 1024,
        },
        &allowed(&["/dev/sdb"]),
        &[],
    )
    .unwrap_err();

    assert_eq!(error.kind, VolumeErrorKind::Busy);
    assert!(error.detail.contains("vfat"), "{}", error.detail);
    assert!(error.detail.contains("destroy"), "{}", error.detail);
    assert!(
        runner.calls().is_empty(),
        "the guard must refuse before any call is made"
    );
}

#[test]
fn a_disk_without_a_filesystem_is_still_accepted() {
    let snap = snapshot(vec![disk("sdb", "/dev/sdb")], Vec::new());
    let runner = FakeRunner::ok();
    create_with(
        &runner,
        &snap,
        &PartitionCreateArgs {
            disk: "/dev/sdb".to_string(),
            kind: "linux".to_string(),
            start_bytes: 0,
            size_bytes: 1024 * 1024,
        },
        &allowed(&["/dev/sdb"]),
        &[],
    )
    .expect("a truly blank disk must still accept a partition");
    assert_eq!(runner.calls().len(), 1);
}

#[test]
fn a_partitioned_disk_with_a_filesystem_is_not_mistaken_for_a_superfloppy() {
    // lsblk reports the fstype on partitions, not on the parent disk. A disk
    // whose child carries ext4 must still be editable.
    let mut parent = disk("sdb", "/dev/sdb");
    parent.fstype = None;
    let snap = snapshot(vec![parent, part("sdb1", "/dev/sdb1", "sdb")], Vec::new());
    let runner = FakeRunner::ok();
    create_with(
        &runner,
        &snap,
        &PartitionCreateArgs {
            disk: "/dev/sdb".to_string(),
            kind: "linux".to_string(),
            start_bytes: 0,
            size_bytes: 1024 * 1024,
        },
        &allowed(&["/dev/sdb"]),
        &[],
    )
    .expect("a partitioned disk must stay editable");
    assert_eq!(
        runner.calls()[0].1,
        vec![
            "--no-reread".to_string(),
            "--append".to_string(),
            "/dev/sdb".to_string()
        ]
    );
}

#[test]
fn an_active_swap_area_is_refused() {
    // Swap is never in /proc/self/mountinfo, and --no-reread disables sfdisk's
    // own in-use check, so this guard is the only thing in the way.
    let mut snap = snapshot(vec![disk("sdb", "/dev/sdb")], Vec::new());
    snap.swap_devices = vec!["/dev/sdb2".to_string()];
    let runner = FakeRunner::ok();
    let error = create_with(
        &runner,
        &snap,
        &PartitionCreateArgs {
            disk: "/dev/sdb".to_string(),
            kind: "linux".to_string(),
            start_bytes: 0,
            size_bytes: 1024 * 1024,
        },
        &allowed(&["/dev/sdb"]),
        &[],
    )
    .unwrap_err();

    assert_eq!(error.kind, VolumeErrorKind::Busy);
    assert!(error.detail.contains("swap"), "{}", error.detail);
    assert!(runner.calls().is_empty());
}

#[test]
fn a_real_sfdisk_would_destroy_a_whole_disk_filesystem() {
    // Proves the guard is not defending against an imaginary hazard: writing a
    // table over a vfat image with no --append really does rewrite sector 0.
    let path = path_with_sbin();
    if !have("sfdisk") || !have("mkfs.vfat") {
        eprintln!("skipping: sfdisk or mkfs.vfat is not installed");
        return;
    }
    let dir = std::env::temp_dir().join(format!("anth-part-superfloppy-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let image = dir.join("usb.img");
    let file = std::fs::File::create(&image).unwrap();
    file.set_len(64 * 1024 * 1024).unwrap();
    drop(file);

    // No `-q`: mkfs.fat rejects it, and a rejected command would make this
    // test skip itself and assert nothing.
    let mkfs = std::process::Command::new("mkfs.vfat")
        .arg(&image)
        .env("PATH", &path)
        .output()
        .expect("mkfs.vfat is on the PATH built above");
    assert!(
        mkfs.status.success(),
        "mkfs.vfat must succeed, or this test proves nothing: {}",
        String::from_utf8_lossy(&mkfs.stderr)
    );

    let before = std::fs::read(&image).unwrap()[446..462].to_vec();
    assert_eq!(before, vec![0u8; 16], "a fresh vfat image has no MBR entry");

    let call = plan_edit(
        &image.display().to_string(),
        &TableEdit::Create {
            kind: PartitionKind::Linux,
            start_bytes: 0,
            size_bytes: 1024 * 1024,
        },
        false,
    )
    .unwrap();
    let output = run_real(&call, &path);
    assert!(
        output.status.success(),
        "sfdisk reports success while destroying it"
    );

    let after = std::fs::read(&image).unwrap()[446..462].to_vec();
    assert_ne!(
        after, before,
        "writing a table over a superfloppy must change the MBR area, which is \
         why the guard exists"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn formatting_an_active_swap_partition_is_refused() {
    // Swap is not mounted, so the mount check cannot see it, and `mkfs.fat`
    // overwrites a live swap signature with exit 0. Losing a swap area is the
    // least recoverable damage this feature can do.
    let mut snap = snapshot(
        vec![disk("sdb", "/dev/sdb"), part("sdb1", "/dev/sdb1", "sdb")],
        Vec::new(),
    );
    snap.swap_devices = vec!["/dev/sdb1".to_string()];
    let runner = FakeRunner::ok();
    let error = edit_with(
        &runner,
        &snap,
        &PartitionTableAction::Format {
            partition: "/dev/sdb1".to_string(),
            fs_type: "vfat".to_string(),
            label: None,
        },
        &allowed(&["/dev/sdb"]),
        &[],
        true,
    )
    .unwrap_err();

    assert_eq!(error.kind, VolumeErrorKind::Busy);
    assert!(error.detail.contains("swap"), "{}", error.detail);
    assert!(
        runner.calls().is_empty(),
        "the guard must refuse before mkfs runs"
    );
}

#[test]
fn formatting_a_partition_whose_sibling_is_swap_is_allowed() {
    // Exact match, not `mount_covers`: a swapped sibling must not block
    // formatting this partition.
    let mut snap = snapshot(
        vec![
            disk("sdb", "/dev/sdb"),
            part("sdb1", "/dev/sdb1", "sdb"),
            part("sdb2", "/dev/sdb2", "sdb"),
        ],
        Vec::new(),
    );
    snap.swap_devices = vec!["/dev/sdb2".to_string()];
    let runner = FakeRunner::ok();
    edit_with(
        &runner,
        &snap,
        &PartitionTableAction::Format {
            partition: "/dev/sdb1".to_string(),
            fs_type: "vfat".to_string(),
            label: None,
        },
        &allowed(&["/dev/sdb"]),
        &[],
        true,
    )
    .expect("a swap sibling must not block formatting another partition");
    assert_eq!(runner.calls().len(), 1);
}

#[test]
fn formatting_a_whole_disk_swap_area_is_refused() {
    // A superfloppy signed by `mkswap` has no partitions, so a format request
    // naming the disk as its own target must still be caught.
    let mut snap = snapshot(vec![disk("sdb", "/dev/sdb")], Vec::new());
    snap.swap_devices = vec!["/dev/sdb".to_string()];
    let error =
        anth_panel::volumes::refuse_active_swap(&snap, &["/dev/sdb1", "/dev/sdb"]).unwrap_err();
    assert_eq!(error.kind, VolumeErrorKind::Busy);
    assert!(error.detail.contains("/dev/sdb"), "{}", error.detail);
}
