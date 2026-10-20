use std::cell::RefCell;
use std::path::{Path, PathBuf};

use anth_panel::command::{CommandError, CommandOutput, CommandRunner};
use anth_panel::volumes::{
    MountArgs, Probe, collect_with, parse_lsblk, parse_mountinfo, usage_for, validate_mount,
    validate_unmount,
};
use anth_proto::{VolumeErrorKind, VolumeKind, VolumeSnapshot, VolumeUsage};

const LSBLK_MODERN: &str = r#"{
   "blockdevices": [
      {
         "name": "sda",
         "path": "/dev/sda",
         "type": "disk",
         "size": 80530636800,
         "fstype": null,
         "label": null,
         "uuid": null,
         "mountpoints": [null],
         "ro": false,
         "rm": false,
         "model": "QEMU HARDDISK",
         "tran": "sata",
         "children": [
            {
               "name": "sda1",
               "path": "/dev/sda1",
               "type": "part",
               "size": 80396402176,
               "fstype": "ext4",
               "label": "root",
               "uuid": "1111-2222",
               "mountpoints": ["/"],
               "ro": false,
               "rm": false,
               "model": null,
               "tran": null
            }
         ]
      },
      {
         "name": "sdb",
         "path": "/dev/sdb",
         "type": "disk",
         "size": 16000000000,
         "fstype": "vfat",
         "label": null,
         "uuid": null,
         "mountpoints": [],
         "ro": false,
         "rm": true,
         "model": "USB DISK",
         "tran": "usb",
         "children": []
      }
   ]
}"#;

const LSBLK_LEGACY: &str = r#"{
   "blockdevices": [
      {
         "name": "vda",
         "type": "disk",
         "size": "10737418240",
         "fstype": "xfs",
         "label": "data",
         "uuid": null,
         "mountpoint": "/mnt/data",
         "ro": "1",
         "rm": "0"
      }
   ]
}"#;

const MOUNTINFO: &str = "\
751 712 0:56 / / rw,relatime - overlay overlay rw
753 751 0:108 / /proc rw,nosuid,nodev,noexec,relatime - proc proc rw
760 751 8:1 / /mnt/my\\040disk rw,relatime - ext4 /dev/sda1 rw
761 751 0:50 / /run ro,nosuid - tmpfs tmpfs rw
762 751 9:9 / /srv/share rw,relatime shared:1 - nfs4 server:/export rw
";

struct FakeProbe {
    lsblk: Result<String, Box<str>>,
    mountinfo: String,
}

impl FakeProbe {
    fn new(lsblk: &str, mountinfo: &str) -> Self {
        Self {
            lsblk: Ok(lsblk.to_string()),
            mountinfo: mountinfo.to_string(),
        }
    }
}

impl Probe for FakeProbe {
    fn lsblk(&self) -> Result<String, anth_proto::VolumeError> {
        match &self.lsblk {
            Ok(text) => Ok(text.clone()),
            Err(detail) => Err(anth_proto::VolumeError::new(
                VolumeErrorKind::Failed,
                detail.to_string(),
            )),
        }
    }

    fn mountinfo(&self) -> Result<String, anth_proto::VolumeError> {
        Ok(self.mountinfo.clone())
    }

    fn usage(&self, _mountpoint: &str) -> Option<VolumeUsage> {
        None
    }
}

fn snapshot_from(lsblk: &str, mountinfo: &str) -> VolumeSnapshot {
    collect_with(false, &FakeProbe::new(lsblk, mountinfo))
}

fn scratch_dir(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("anth-vol-{name}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn parse_lsblk_flattens_the_tree_with_parent_links() {
    let devices = parse_lsblk(LSBLK_MODERN).unwrap();
    let names: Vec<&str> = devices.iter().map(|device| device.name.as_str()).collect();

    assert_eq!(names, vec!["sda", "sda1", "sdb"]);
    assert_eq!(devices[0].parent, None);
    assert_eq!(devices[1].parent.as_deref(), Some("sda"));
    assert_eq!(devices[1].kind, VolumeKind::Part);
    assert_eq!(devices[0].kind, VolumeKind::Disk);
    assert_eq!(devices[1].mountpoints, vec!["/".to_string()]);
    assert!(!devices[1].removable);
    assert!(devices[2].removable);
}

#[test]
fn parse_lsblk_reads_strings_numbers_and_missing_fields() {
    let devices = parse_lsblk(LSBLK_LEGACY).unwrap();
    let device = &devices[0];

    assert_eq!(device.size_bytes, 10_737_418_240);
    assert_eq!(device.fstype.as_deref(), Some("xfs"));
    assert_eq!(device.label.as_deref(), Some("data"));
    assert_eq!(device.uuid, None);
    assert_eq!(device.mountpoints, vec!["/mnt/data".to_string()]);
    assert!(device.read_only);
    assert!(!device.removable);
    // A legacy document has no PATH, so it falls back to /dev/<name>.
    assert_eq!(device.path, "/dev/vda");
}

#[test]
fn parse_lsblk_rejects_a_document_without_blockdevices() {
    let error = parse_lsblk(r#"{"devices": []}"#).unwrap_err();
    assert_eq!(error.kind, VolumeErrorKind::Invalid);

    let error = parse_lsblk("not json").unwrap_err();
    assert_eq!(error.kind, VolumeErrorKind::Invalid);
}

#[test]
fn parse_mountinfo_unescapes_and_flags_virtual_filesystems() {
    let mounts = parse_mountinfo(MOUNTINFO);
    assert_eq!(mounts.len(), 5);

    assert_eq!(mounts[0].mountpoint, "/");
    assert_eq!(mounts[0].fstype, "overlay");
    assert!(mounts[0].virtual_fs);

    assert_eq!(mounts[1].mountpoint, "/proc");
    assert!(mounts[1].virtual_fs);

    assert_eq!(mounts[2].mountpoint, "/mnt/my disk");
    assert_eq!(mounts[2].source, "/dev/sda1");
    assert!(!mounts[2].virtual_fs);

    assert_eq!(mounts[3].mountpoint, "/run");
    assert!(mounts[3].read_only);
    assert!(mounts[3].virtual_fs);

    assert_eq!(mounts[4].fstype, "nfs4");
    assert_eq!(mounts[4].source, "server:/export");
    assert!(!mounts[4].virtual_fs);
}

#[test]
fn parse_mountinfo_preserves_non_ascii_paths() {
    // The escaped form is the multi-byte UTF-8 sequence written byte by byte,
    // which is what the kernel emits for a mountpoint outside ASCII.
    let escaped = "1 0 8:1 / /mnt/D\\303\\244tos rw - ext4 /dev/sda1 rw\n";
    let mounts = parse_mountinfo(escaped);
    assert_eq!(mounts.len(), 1);
    assert_eq!(mounts[0].mountpoint, "/mnt/Dätos");

    // An unescaped non-ASCII path arrives as UTF-8 bytes and must survive too.
    let raw = "1 0 8:1 / /mnt/Dätos rw - ext4 /dev/sda1 rw\n";
    assert_eq!(parse_mountinfo(raw)[0].mountpoint, "/mnt/Dätos");
}

#[test]
fn a_non_ascii_mountpoint_passes_validation() {
    let mountinfo = "1 0 8:1 / /mnt/D\\303\\244tos rw - ext4 /dev/sda1 rw\n";
    let snapshot = snapshot_from(LSBLK_MODERN, mountinfo);
    assert_eq!(snapshot.mounts[0].mountpoint, "/mnt/Dätos");

    // The same path the kernel reported must match byte for byte here,
    // otherwise a real non-ASCII mount can never be unmounted.
    let resolved = validate_unmount(&snapshot, "/mnt/Dätos", &[]).unwrap();
    assert_eq!(resolved, Path::new("/mnt/Dätos"));
}

#[test]
fn parse_mountinfo_skips_malformed_lines() {
    let text = "garbage\n\n1 2 3 4 5 6 7\n";
    assert!(parse_mountinfo(text).is_empty());
}

#[test]
fn parse_mountinfo_requires_the_separator() {
    // Six fields but no `-`, which is what a truncated line looks like.
    assert!(parse_mountinfo("1 2 0:1 / /mnt rw,relatime").is_empty());
}

#[test]
fn collect_reports_devices_and_mounts_together() {
    let snapshot = snapshot_from(LSBLK_MODERN, MOUNTINFO);

    assert_eq!(snapshot.devices.len(), 3);
    assert_eq!(snapshot.mounts.len(), 5);
    assert_eq!(snapshot.total, 3);
    assert!(!snapshot.truncated);
    assert!(snapshot.warning.is_none());
    assert!(snapshot.timestamp_ms > 0);
}

#[test]
fn a_missing_lsblk_degrades_to_mounts_only() {
    let probe = FakeProbe {
        lsblk: Err("lsblk is not installed or is not on PATH".into()),
        mountinfo: MOUNTINFO.to_string(),
    };
    let snapshot = collect_with(false, &probe);

    assert!(snapshot.devices.is_empty());
    assert_eq!(snapshot.mounts.len(), 5);
    let warning = snapshot.warning.expect("a warning should explain the gap");
    assert!(warning.contains("lsblk"), "unexpected warning: {warning}");
}

#[test]
fn an_unparseable_lsblk_keeps_the_mounts() {
    let snapshot = snapshot_from("not json", MOUNTINFO);
    assert!(snapshot.devices.is_empty());
    assert_eq!(snapshot.mounts.len(), 5);
    assert!(snapshot.warning.is_some());
}

#[test]
fn validate_mount_accepts_an_unmounted_device_inside_the_root() {
    let snapshot = snapshot_from(LSBLK_MODERN, MOUNTINFO);
    let root = scratch_dir("mount-ok");
    let target = root.join("data");
    std::fs::create_dir_all(&target).unwrap();

    let validated = validate_mount(
        &snapshot,
        "/dev/sdb",
        target.to_str().unwrap(),
        None,
        false,
        true,
        Some(&root),
    )
    .unwrap();

    assert_eq!(validated.device, "/dev/sdb");
    assert_eq!(validated.fs_type, "vfat");
    assert_eq!(validated.target, std::fs::canonicalize(&target).unwrap());

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn validate_mount_accepts_a_kernel_name() {
    let snapshot = snapshot_from(LSBLK_MODERN, MOUNTINFO);
    let root = scratch_dir("mount-name");
    let target = root.join("data");
    std::fs::create_dir_all(&target).unwrap();

    let validated = validate_mount(
        &snapshot,
        "sdb",
        target.to_str().unwrap(),
        Some("vfat"),
        true,
        true,
        Some(&root),
    )
    .unwrap();

    assert_eq!(validated.fs_type, "vfat");
    assert!(validated.read_only);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn validate_mount_rejects_an_unknown_device() {
    let snapshot = snapshot_from(LSBLK_MODERN, MOUNTINFO);
    let error =
        validate_mount(&snapshot, "/dev/sdz", "/mnt", None, false, false, None).unwrap_err();
    assert_eq!(error.kind, VolumeErrorKind::NotFound);
}

#[test]
fn validate_mount_rejects_an_already_mounted_device() {
    let snapshot = snapshot_from(LSBLK_MODERN, MOUNTINFO);
    let error =
        validate_mount(&snapshot, "/dev/sda1", "/mnt", None, false, false, None).unwrap_err();
    assert_eq!(error.kind, VolumeErrorKind::Invalid);
    assert!(error.detail.contains("already mounted"));
}

#[test]
fn validate_mount_rejects_removable_media_without_the_flag() {
    let snapshot = snapshot_from(LSBLK_MODERN, MOUNTINFO);
    let root = scratch_dir("mount-removable");
    let target = root.join("usb");
    std::fs::create_dir_all(&target).unwrap();

    let error = validate_mount(
        &snapshot,
        "/dev/sdb",
        target.to_str().unwrap(),
        None,
        false,
        false,
        Some(&root),
    )
    .unwrap_err();
    assert_eq!(error.kind, VolumeErrorKind::Protected);

    validate_mount(
        &snapshot,
        "/dev/sdb",
        target.to_str().unwrap(),
        None,
        false,
        true,
        Some(&root),
    )
    .unwrap();

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn validate_mount_enforces_the_mount_root() {
    let snapshot = snapshot_from(LSBLK_MODERN, MOUNTINFO);
    let root = scratch_dir("mount-root");
    let outside = scratch_dir("mount-outside");

    let error = validate_mount(
        &snapshot,
        "/dev/sdb",
        outside.to_str().unwrap(),
        None,
        false,
        true,
        Some(&root),
    )
    .unwrap_err();
    assert_eq!(error.kind, VolumeErrorKind::Protected);

    // An empty ceiling is the documented escape hatch.
    validate_mount(
        &snapshot,
        "/dev/sdb",
        outside.to_str().unwrap(),
        None,
        false,
        true,
        None,
    )
    .unwrap();

    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&outside);
}

#[test]
fn validate_mount_rejects_a_missing_or_relative_target() {
    let snapshot = snapshot_from(LSBLK_MODERN, MOUNTINFO);

    let error =
        validate_mount(&snapshot, "/dev/sdb", "mnt/data", None, false, true, None).unwrap_err();
    assert_eq!(error.kind, VolumeErrorKind::Invalid);

    let error = validate_mount(
        &snapshot,
        "/dev/sdb",
        "/definitely/not/here",
        None,
        false,
        true,
        None,
    )
    .unwrap_err();
    assert_eq!(error.kind, VolumeErrorKind::NotFound);

    let error = validate_mount(
        &snapshot,
        "/dev/sdb",
        "/tmp/../etc",
        None,
        false,
        true,
        None,
    )
    .unwrap_err();
    assert_eq!(error.kind, VolumeErrorKind::Invalid);
}

#[test]
fn validate_mount_refuses_a_device_without_a_filesystem() {
    let snapshot = snapshot_from(LSBLK_MODERN, MOUNTINFO);
    let root = scratch_dir("mount-nofs");
    let target = root.join("raw");
    std::fs::create_dir_all(&target).unwrap();

    let error = validate_mount(
        &snapshot,
        "/dev/sda",
        target.to_str().unwrap(),
        None,
        false,
        false,
        Some(&root),
    )
    .unwrap_err();
    assert_eq!(error.kind, VolumeErrorKind::Invalid);
    assert!(error.detail.contains("--type"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn validate_mount_rejects_an_injected_filesystem_type() {
    let snapshot = snapshot_from(LSBLK_MODERN, MOUNTINFO);
    let root = scratch_dir("mount-inject");
    let target = root.join("data");
    std::fs::create_dir_all(&target).unwrap();

    for bad in ["ext4 -o bind", "ext4;rm"] {
        let error = validate_mount(
            &snapshot,
            "/dev/sdb",
            target.to_str().unwrap(),
            Some(bad),
            false,
            true,
            Some(&root),
        )
        .unwrap_err();
        assert_eq!(error.kind, VolumeErrorKind::Invalid, "accepted `{bad}`");
    }

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn validate_unmount_accepts_a_real_mount() {
    let snapshot = snapshot_from(LSBLK_MODERN, MOUNTINFO);
    let resolved = validate_unmount(&snapshot, "/mnt/my disk", &[]).unwrap();
    assert_eq!(resolved, Path::new("/mnt/my disk"));
}

#[test]
fn validate_unmount_refuses_the_root() {
    let snapshot = snapshot_from(LSBLK_MODERN, MOUNTINFO);
    let error = validate_unmount(&snapshot, "/", &[]).unwrap_err();
    assert_eq!(error.kind, VolumeErrorKind::Protected);
}

#[test]
fn validate_unmount_refuses_a_virtual_filesystem() {
    let snapshot = snapshot_from(LSBLK_MODERN, MOUNTINFO);
    for mountpoint in ["/proc", "/run", "/"] {
        let error = validate_unmount(&snapshot, mountpoint, &[]).unwrap_err();
        assert_eq!(error.kind, VolumeErrorKind::Protected, "{mountpoint}");
    }
}

#[test]
fn validate_unmount_refuses_a_path_that_is_not_a_mountpoint() {
    let snapshot = snapshot_from(LSBLK_MODERN, MOUNTINFO);
    let error = validate_unmount(&snapshot, "/etc", &[]).unwrap_err();
    assert_eq!(error.kind, VolumeErrorKind::NotFound);

    let error = validate_unmount(&snapshot, "mnt/my disk", &[]).unwrap_err();
    assert_eq!(error.kind, VolumeErrorKind::Invalid);
}

#[test]
fn validate_unmount_refuses_an_ancestor_of_the_panel_paths() {
    let snapshot = snapshot_from(LSBLK_MODERN, MOUNTINFO);
    let guarded = PathBuf::from("/mnt/my disk/anth.sock");
    let error = validate_unmount(&snapshot, "/mnt/my disk", &[guarded]).unwrap_err();
    assert_eq!(error.kind, VolumeErrorKind::Protected);
    assert!(error.detail.contains("which the panel needs"));
}

#[test]
fn a_symlinked_target_cannot_escape_the_mount_root() {
    let snapshot = snapshot_from(LSBLK_MODERN, MOUNTINFO);
    let root = scratch_dir("mount-symlink");
    let outside = scratch_dir("mount-symlink-outside");
    let link = root.join("escape");
    std::os::unix::fs::symlink(&outside, &link).unwrap();

    let error = validate_mount(
        &snapshot,
        "/dev/sdb",
        link.to_str().unwrap(),
        None,
        false,
        true,
        Some(&root),
    )
    .unwrap_err();
    assert_eq!(error.kind, VolumeErrorKind::Protected);

    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&outside);
}

struct RecordingRunner {
    calls: RefCell<Vec<(String, Vec<String>)>>,
    result: Result<CommandOutput, CommandError>,
}

impl RecordingRunner {
    fn ok() -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
            result: Ok(CommandOutput {
                ok: true,
                stdout: String::new(),
                stderr: String::new(),
            }),
        }
    }

    fn failing(stderr: &str) -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
            result: Ok(CommandOutput {
                ok: false,
                stdout: String::new(),
                stderr: stderr.to_string(),
            }),
        }
    }

    fn calls(&self) -> Vec<(String, Vec<String>)> {
        self.calls.borrow().clone()
    }
}

impl CommandRunner for RecordingRunner {
    fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput, CommandError> {
        self.calls.borrow_mut().push((
            program.to_string(),
            args.iter().map(|arg| (*arg).to_string()).collect(),
        ));
        self.result.clone()
    }

    fn run_with_stdin(
        &self,
        program: &str,
        args: &[&str],
        _input: &str,
    ) -> Result<CommandOutput, CommandError> {
        self.run(program, args)
    }
}

#[test]
fn mount_runs_the_expected_argument_vector() {
    let root = scratch_dir("mount-run");
    let target = root.join("data");
    std::fs::create_dir_all(&target).unwrap();
    let probe = FakeProbe::new(LSBLK_MODERN, MOUNTINFO);
    let runner = RecordingRunner::ok();

    let message = anth_panel::volumes::mount_with(
        true,
        &probe,
        &runner,
        Some(&root),
        &MountArgs {
            device: "/dev/sdb",
            target: target.to_str().unwrap(),
            fs_type: Some("ext4"),
            read_only: true,
            allow_removable: true,
        },
    )
    .unwrap();

    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "mount");
    assert_eq!(
        calls[0].1,
        vec![
            "-o".to_string(),
            "ro".to_string(),
            "-t".to_string(),
            "ext4".to_string(),
            "/dev/sdb".to_string(),
            std::fs::canonicalize(&target)
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        ]
    );
    assert!(message.starts_with("mounted /dev/sdb on"));
    assert!(message.ends_with("as ext4"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn mount_is_refused_when_control_is_disabled() {
    let probe = FakeProbe::new(LSBLK_MODERN, MOUNTINFO);
    let runner = RecordingRunner::ok();

    let error = anth_panel::volumes::mount_with(
        false,
        &probe,
        &runner,
        None,
        &MountArgs {
            device: "/dev/sdb",
            target: "/mnt",
            fs_type: None,
            read_only: false,
            allow_removable: false,
        },
    )
    .unwrap_err();

    assert_eq!(error.kind, VolumeErrorKind::Disabled);
    assert!(
        runner.calls().is_empty(),
        "nothing should have been executed"
    );
}

#[test]
fn unmount_reports_a_busy_filesystem() {
    let probe = FakeProbe::new(LSBLK_MODERN, MOUNTINFO);
    let runner = RecordingRunner::failing("umount: /mnt/my disk: target is busy.");

    let error =
        anth_panel::volumes::unmount_with(true, &probe, &runner, &[], "/mnt/my disk").unwrap_err();

    assert_eq!(error.kind, VolumeErrorKind::Busy);
    assert_eq!(runner.calls()[0].0, "umount");
}

#[test]
fn unmount_maps_permission_failures() {
    let probe = FakeProbe::new(LSBLK_MODERN, MOUNTINFO);
    let runner = RecordingRunner::failing("umount: only root can do that");

    let error =
        anth_panel::volumes::unmount_with(true, &probe, &runner, &[], "/mnt/my disk").unwrap_err();

    assert_eq!(error.kind, VolumeErrorKind::PermissionDenied);
}

#[test]
fn usage_reads_the_root_filesystem() {
    let usage = usage_for("/").expect("/ must be statvfs-able");
    assert!(usage.total_bytes > 0);
    assert!(usage.used_bytes <= usage.total_bytes);
}

#[test]
fn usage_of_a_missing_path_is_none() {
    assert!(usage_for("/definitely/not/mounted").is_none());
}

#[test]
fn a_large_snapshot_is_shortened_under_the_line_cap() {
    let mut devices = Vec::new();
    for index in 0..20_000 {
        devices.push(anth_proto::VolumeDevice {
            name: format!("loop{index}"),
            path: format!("/dev/loop{index}"),
            kind: VolumeKind::Loop,
            model: Some("A rather long model string that costs line budget".repeat(4)),
            uuid: Some("11111111-2222-3333-4444-555555555555".to_string()),
            ..Default::default()
        });
    }
    let snapshot = VolumeSnapshot {
        devices,
        mounts: Vec::new(),
        control_enabled: false,
        timestamp_ms: 1,
        total: 0,
        truncated: false,
        warning: None,
    };

    let fitted = anth_panel::volumes::fit_snapshot_for_test(snapshot);

    assert!(fitted.truncated);
    assert!(fitted.devices.len() < 20_000);
    assert_eq!(fitted.total, 20_000);
    let json = anth_proto::Response::VolumeSnapshot(fitted.clone())
        .to_json()
        .unwrap();
    assert!(
        json.len() <= anth_proto::MAX_LINE_LEN,
        "the framed response is what has to fit: {} bytes",
        json.len()
    );
    assert!(fitted.warning.unwrap().contains("showing"));
}

#[test]
fn mutations_see_devices_that_truncation_would_drop() {
    // One device would fit; the guard must not depend on the line cap, so the
    // last device is still reachable after the snapshot is shortened.
    let mut devices = Vec::new();
    for index in 0..20_000 {
        devices.push(anth_proto::VolumeDevice {
            name: format!("loop{index}"),
            path: format!("/dev/loop{index}"),
            kind: VolumeKind::Loop,
            fstype: Some("ext4".to_string()),
            model: Some("A rather long model string that costs line budget".repeat(4)),
            ..Default::default()
        });
    }
    let probe = FakeProbe::new(
        &serde_json::to_string(&serde_json::json!({
            "blockdevices": devices.iter().map(|device| serde_json::json!({
                "name": device.name,
                "path": device.path,
                "type": "loop",
                "size": 1024,
                "fstype": "ext4",
                "mountpoints": [],
                "ro": false,
                "rm": false,
                "model": device.model,
            })).collect::<Vec<_>>()
        }))
        .unwrap(),
        "",
    );

    let snapshot = collect_with(true, &probe);
    assert!(snapshot.truncated, "the fixture must exceed the line cap");
    assert!(snapshot.devices.len() < devices.len());
    assert!(
        !snapshot
            .devices
            .iter()
            .any(|device| device.name == "loop19999"),
        "the fixture must drop the device under test from the fitted snapshot"
    );

    let root = scratch_dir("unfitted");
    let target = root.join("last");
    std::fs::create_dir_all(&target).unwrap();
    let runner = RecordingRunner::ok();

    // `loop19999` is past the truncation point. Validating against the fitted
    // snapshot would refuse it as unknown.
    anth_panel::volumes::mount_with(
        true,
        &probe,
        &runner,
        Some(&root),
        &MountArgs {
            device: "/dev/loop19999",
            target: target.to_str().unwrap(),
            fs_type: None,
            read_only: false,
            allow_removable: false,
        },
    )
    .unwrap();
    assert_eq!(runner.calls()[0].0, "mount");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_small_snapshot_is_not_marked_truncated() {
    let fitted = anth_panel::volumes::fit_snapshot_for_test(snapshot_from(LSBLK_MODERN, MOUNTINFO));
    assert!(!fitted.truncated);
    assert!(fitted.warning.is_none());
    assert_eq!(fitted.total, 3);
}
