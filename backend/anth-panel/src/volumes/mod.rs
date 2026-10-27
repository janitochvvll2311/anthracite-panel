//! Storage inventory: block devices, mounts and swap areas.
//!
//! The read path is deliberately read-only. Mount, partition and format
//! operations arrive in later slices and validate against a fresh snapshot.

mod parse;

use std::ffi::CString;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anth_wire::{
    ErrorKind, VolumeMountRequest, VolumeSnapshot, VolumeUnmountRequest, VolumeUsage, WireError,
};

use crate::config::VolumesConfig;
use parse::{LSBLK_COLUMNS, LSBLK_COLUMNS_LEGACY, parse_lsblk, parse_mountinfo, swaps_from};

/// Everything the inventory needs from the host, behind a trait so tests can
/// script it without spawning `lsblk` or touching the real mounts.
pub trait Probe: Send + Sync {
    fn lsblk(&self) -> Result<String, String>;
    fn mountinfo(&self) -> Result<String, String>;
    /// `None` means `/proc/swaps` could not be read; callers must not treat it
    /// as an empty list.
    fn swaps(&self) -> Option<Vec<String>>;
    fn usage(&self, mountpoint: &str) -> Option<VolumeUsage>;
}

/// The real host probe.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemProbe;

impl Probe for SystemProbe {
    fn lsblk(&self) -> Result<String, String> {
        let modern = run_lsblk(LSBLK_COLUMNS);
        match modern {
            Ok(text) => Ok(text),
            Err(modern) => run_lsblk(LSBLK_COLUMNS_LEGACY)
                .map_err(|legacy| format!("{modern}; legacy: {legacy}")),
        }
    }

    fn mountinfo(&self) -> Result<String, String> {
        std::fs::read_to_string("/proc/self/mountinfo")
            .map_err(|error| format!("cannot read /proc/self/mountinfo: {error}"))
    }

    fn swaps(&self) -> Option<Vec<String>> {
        std::fs::read_to_string("/proc/swaps")
            .ok()
            .map(|text| swaps_from(&text))
    }

    fn usage(&self, mountpoint: &str) -> Option<VolumeUsage> {
        usage_for(mountpoint)
    }
}

/// One `mount` or `umount` result, decoupled from `std::process::Output`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountOutput {
    pub ok: bool,
    pub stderr: String,
}

/// Runs `mount`/`umount` with an argument vector. Tests implement this trait.
pub trait MountRunner: Send + Sync {
    /// `Err` means the program could not be executed; a refusal is
    /// `Ok(MountOutput { ok: false, .. })`.
    fn run(&self, program: &str, args: &[String]) -> Result<MountOutput, String>;
}

/// The real `mount`/`umount` binaries, invoked directly.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemMountRunner;

impl MountRunner for SystemMountRunner {
    fn run(&self, program: &str, args: &[String]) -> Result<MountOutput, String> {
        let output = Command::new(program)
            .args(args)
            .output()
            .map_err(|error| error.to_string())?;
        Ok(MountOutput {
            ok: output.status.success(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        })
    }
}

fn run_lsblk(columns: &str) -> Result<String, String> {
    let output = Command::new("lsblk")
        .args(["-J", "-b", "-o", columns])
        .output()
        .map_err(|error| format!("lsblk is not installed or could not be executed: {error}"))?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(if stderr.is_empty() {
        format!("lsblk exited with {}", output.status)
    } else {
        format!("lsblk failed: {stderr}")
    })
}

/// One snapshot. A failing `lsblk` or unreadable mountinfo degrades to the
/// other source with a warning instead of failing the whole request; a minimal
/// container without `lsblk` can still show its mounts.
#[derive(Clone)]
pub struct VolumesService {
    config: VolumesConfig,
    probe: Arc<dyn Probe>,
    runner: Arc<dyn MountRunner>,
    protected: Vec<PathBuf>,
}

impl VolumesService {
    pub fn new(config: VolumesConfig, protected: Vec<PathBuf>) -> Self {
        Self::with_parts(
            config,
            Arc::new(SystemProbe),
            Arc::new(SystemMountRunner),
            protected,
        )
    }

    /// Replaces only the inventory probe. Used by inventory tests.
    pub fn with_probe(config: VolumesConfig, probe: Arc<dyn Probe>) -> Self {
        Self::with_parts(config, probe, Arc::new(SystemMountRunner), Vec::new())
    }

    /// Replaces the probe and the command runner. Used by mutation tests.
    pub fn with_parts(
        config: VolumesConfig,
        probe: Arc<dyn Probe>,
        runner: Arc<dyn MountRunner>,
        protected: Vec<PathBuf>,
    ) -> Self {
        Self {
            config,
            probe,
            runner,
            protected,
        }
    }

    pub fn snapshot(&self) -> VolumeSnapshot {
        let mut warnings = Vec::new();

        let mut devices = match self.probe.lsblk() {
            Ok(text) => match parse_lsblk(&text) {
                Ok(devices) => devices,
                Err(error) => {
                    warnings.push(format!("device inventory unavailable: {error}"));
                    Vec::new()
                }
            },
            Err(error) => {
                warnings.push(format!("device inventory unavailable: {error}"));
                Vec::new()
            }
        };

        let mut mounts = match self.probe.mountinfo() {
            Ok(text) => parse_mountinfo(&text),
            Err(error) => {
                warnings.push(format!("mount inventory unavailable: {error}"));
                Vec::new()
            }
        };
        for mount in &mut mounts {
            mount.usage = self.probe.usage(&mount.mountpoint);
        }

        // lsblk reports a null fstype for a bind-mounted host partition on
        // some kernels; the mount source still knows the filesystem.
        for device in &mut devices {
            if device.fstype.is_none()
                && let Some(mount) = mounts
                    .iter()
                    .find(|mount| mount.source == device.path && !mount.virtual_fs)
            {
                device.fstype = Some(mount.fstype.clone());
            }
        }

        let swap_devices = match self.probe.swaps() {
            Some(paths) => paths,
            None => {
                warnings.push("swap areas unavailable; in-use checks are incomplete".to_string());
                Vec::new()
            }
        };

        VolumeSnapshot {
            devices,
            mounts,
            swap_devices,
            control_enabled: self.config.control,
            partition_enabled: self.config.partition,
            format_enabled: self.config.format,
            mount_root: self.config.mount_root.clone(),
            disk_allow: self.config.disk_allow.clone(),
            warning: (!warnings.is_empty()).then(|| warnings.join("; ")),
            timestamp_ms: now_ms(),
        }
    }
}

impl VolumesService {
    /// Mounts one known, unmounted device under the configured mount root.
    pub fn mount(&self, request: &VolumeMountRequest) -> Result<String, WireError> {
        self.require_control()?;
        let snapshot = self.snapshot();
        let device = snapshot
            .devices
            .iter()
            .find(|device| {
                device.path == request.device.trim() || device.name == request.device.trim()
            })
            .ok_or_else(|| {
                WireError::new(
                    ErrorKind::NotFound,
                    "volume_not_found",
                    format!("`{}` is not a block device on this host", request.device),
                )
            })?;
        if device.is_mounted() {
            return Err(WireError::new(
                ErrorKind::Conflict,
                "volume_mounted",
                format!(
                    "`{}` is already mounted on {}",
                    device.path,
                    device.mountpoints.join(", ")
                ),
            ));
        }
        if device.removable && !request.allow_removable {
            return Err(WireError::new(
                ErrorKind::Forbidden,
                "removable_volume",
                format!(
                    "`{}` is removable; allow removable media explicitly to mount it",
                    device.path
                ),
            ));
        }

        let target = require_absolute(&request.target, "target")?;
        let metadata = std::fs::metadata(&target).map_err(|error| {
            WireError::new(
                ErrorKind::NotFound,
                "mount_target_not_found",
                format!("`{}` is not usable: {error}", target.display()),
            )
        })?;
        if !metadata.is_dir() {
            return Err(WireError::new(
                ErrorKind::Invalid,
                "invalid_mount",
                format!("`{}` is not a directory", target.display()),
            ));
        }
        let resolved = std::fs::canonicalize(&target).map_err(|error| {
            WireError::new(
                ErrorKind::NotFound,
                "mount_target_not_found",
                format!("`{}` cannot be resolved: {error}", target.display()),
            )
        })?;
        if snapshot
            .mounts
            .iter()
            .any(|mount| mount.mountpoint == *resolved.to_string_lossy())
        {
            return Err(WireError::new(
                ErrorKind::Conflict,
                "volume_mounted",
                format!("`{}` is already a mountpoint", resolved.display()),
            ));
        }
        let root = self.mount_root()?;
        if !resolved.starts_with(&root) {
            return Err(WireError::new(
                ErrorKind::Forbidden,
                "protected_mount",
                format!(
                    "`{}` is outside the allowed mount root `{}`",
                    resolved.display(),
                    root.display()
                ),
            ));
        }

        let fs_type = request
            .fs_type
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| device.fstype.clone())
            .ok_or_else(|| {
                WireError::new(
                    ErrorKind::Invalid,
                    "invalid_mount",
                    format!(
                        "`{}` has no detected filesystem; pass an explicit type",
                        device.path
                    ),
                )
            })?;
        if !valid_fs_type(&fs_type) {
            return Err(WireError::new(
                ErrorKind::Invalid,
                "invalid_mount",
                format!("invalid filesystem type `{fs_type}`"),
            ));
        }

        let target_text = resolved.to_string_lossy().into_owned();
        let mut args: Vec<String> = Vec::new();
        if request.read_only {
            args.push("-o".to_string());
            args.push("ro".to_string());
        }
        args.push("-t".to_string());
        args.push(fs_type.clone());
        args.push(device.path.clone());
        args.push(target_text.clone());
        self.run_command("mount", &args)?;
        Ok(format!(
            "mounted {} on {} as {}",
            device.path, target_text, fs_type
        ))
    }

    /// Unmounts one current mountpoint, refusing virtual and panel-protected
    /// paths.
    pub fn unmount(&self, request: &VolumeUnmountRequest) -> Result<String, WireError> {
        self.require_control()?;
        let snapshot = self.snapshot();
        let target = require_absolute(&request.target, "target")?;
        if target == Path::new("/") {
            return Err(WireError::new(
                ErrorKind::Forbidden,
                "protected_mount",
                "refusing to unmount `/`",
            ));
        }
        let resolved = std::fs::canonicalize(&target).map_err(|error| {
            WireError::new(
                ErrorKind::NotFound,
                "mount_not_found",
                format!("`{}` is not a mountpoint: {error}", target.display()),
            )
        })?;
        let mount = snapshot
            .mounts
            .iter()
            .find(|mount| mount.mountpoint == *resolved.to_string_lossy())
            .ok_or_else(|| {
                WireError::new(
                    ErrorKind::NotFound,
                    "mount_not_found",
                    format!("`{}` is not a mountpoint", resolved.display()),
                )
            })?;
        if mount.virtual_fs {
            return Err(WireError::new(
                ErrorKind::Forbidden,
                "virtual_mount",
                format!(
                    "`{}` is a `{}` filesystem and cannot be unmounted through the panel",
                    mount.mountpoint, mount.fstype
                ),
            ));
        }
        for guarded in &self.protected {
            let guarded = std::fs::canonicalize(guarded).unwrap_or_else(|_| guarded.clone());
            if guarded.starts_with(&resolved) {
                return Err(WireError::new(
                    ErrorKind::Forbidden,
                    "protected_mount",
                    format!(
                        "`{}` contains `{}`, which the panel needs",
                        resolved.display(),
                        guarded.display()
                    ),
                ));
            }
        }

        let target_text = resolved.to_string_lossy().into_owned();
        self.run_command("umount", std::slice::from_ref(&target_text))?;
        Ok(format!("unmounted {target_text}"))
    }

    fn require_control(&self) -> Result<(), WireError> {
        if self.config.control {
            Ok(())
        } else {
            Err(WireError::new(
                ErrorKind::Forbidden,
                "volumes_control_disabled",
                "volume control is disabled; set `[volumes] control = true`",
            ))
        }
    }

    fn mount_root(&self) -> Result<PathBuf, WireError> {
        let raw = Path::new(&self.config.mount_root);
        if raw.as_os_str().is_empty() {
            return Err(WireError::new(
                ErrorKind::Invalid,
                "invalid_mount",
                "the configured mount root is empty",
            ));
        }
        let path = if raw.is_absolute() {
            raw.to_path_buf()
        } else {
            PathBuf::from("/").join(raw)
        };
        std::fs::canonicalize(&path).map_err(|error| {
            WireError::new(
                ErrorKind::NotFound,
                "mount_root_not_found",
                format!("mount root `{}` is not usable: {error}", path.display()),
            )
        })
    }

    fn run_command(&self, program: &str, args: &[String]) -> Result<(), WireError> {
        match self.runner.run(program, args) {
            Err(detail) => Err(WireError::new(
                ErrorKind::Unsupported,
                "mount_unavailable",
                format!("`{program}` is not installed or could not be executed: {detail}"),
            )),
            Ok(output) if output.ok => Ok(()),
            Ok(output) => Err(classify_failure(program, &output.stderr)),
        }
    }
}

fn require_absolute(value: &str, field: &str) -> Result<PathBuf, WireError> {
    if value.contains('\0') {
        return Err(WireError::new(
            ErrorKind::Invalid,
            "invalid_mount",
            format!("{field} contains a NUL byte"),
        ));
    }
    if !value.starts_with('/') {
        return Err(WireError::new(
            ErrorKind::Invalid,
            "invalid_mount",
            format!("{field} must be an absolute path"),
        ));
    }
    let mut out = PathBuf::from("/");
    for component in Path::new(value).components() {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::ParentDir => {
                return Err(WireError::new(
                    ErrorKind::Invalid,
                    "invalid_mount",
                    format!("{field} cannot contain `..`"),
                ));
            }
            Component::Normal(part) => out.push(part),
            Component::Prefix(_) => {
                return Err(WireError::new(
                    ErrorKind::Invalid,
                    "invalid_mount",
                    format!("{field} is not a Unix path"),
                ));
            }
        }
    }
    Ok(out)
}

fn valid_fs_type(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.')
        })
}

fn classify_failure(program: &str, stderr: &str) -> WireError {
    let lowercase = stderr.to_lowercase();
    let (kind, code) = if lowercase.contains("busy") || lowercase.contains("in use") {
        (ErrorKind::Conflict, "mount_busy")
    } else if lowercase.contains("permission denied")
        || lowercase.contains("operation not permitted")
        || lowercase.contains("must be superuser")
        || lowercase.contains("only root")
    {
        (ErrorKind::Forbidden, "mount_permission")
    } else if lowercase.contains("does not exist")
        || lowercase.contains("no such file")
        || lowercase.contains("not found")
    {
        (ErrorKind::NotFound, "mount_not_found")
    } else if lowercase.contains("wrong fs type")
        || lowercase.contains("unknown filesystem")
        || lowercase.contains("bad option")
        || lowercase.contains("invalid")
    {
        (ErrorKind::Invalid, "mount_refused")
    } else {
        (ErrorKind::Internal, "mount_failed")
    };
    let stderr = stderr
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
        } else if code == "mount_permission" {
            format!("mount changes need CAP_SYS_ADMIN; {program} said: {stderr}")
        } else {
            format!("{program} refused the change: {stderr}")
        },
    )
}

/// Free-space counters for one mountpoint. A vanished or unreadable mountpoint
/// yields `None`; it does not fail the snapshot.
pub fn usage_for(mountpoint: &str) -> Option<VolumeUsage> {
    let path = CString::new(mountpoint).ok()?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(path.as_ptr(), &mut stat) } != 0 {
        return None;
    }
    let block = stat.f_frsize.max(1) as u64;
    let total = block.saturating_mul(stat.f_blocks as u64);
    if total == 0 {
        return None;
    }
    let free = block.saturating_mul(stat.f_bfree as u64);
    Some(VolumeUsage {
        total_bytes: total,
        used_bytes: total.saturating_sub(free),
        available_bytes: block.saturating_mul(stat.f_bavail as u64),
    })
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Mutex;

    use super::*;
    use anth_wire::{VolumeKind, VolumeUsage};

    const LSBLK: &str = r#"{
      "blockdevices": [
        {
          "name": "sda", "path": "/dev/sda", "type": "disk", "size": 1000,
          "mountpoints": [null], "ro": false, "rm": false,
          "children": [
            {
              "name": "sda1", "path": "/dev/sda1", "type": "part", "size": 500,
              "fstype": "ext4", "label": "root", "uuid": "abc",
              "mountpoints": ["/"], "ro": false, "rm": false
            },
            {
              "name": "sda2", "path": "/dev/sda2", "type": "part", "size": 200,
              "fstype": "ext4", "label": null, "uuid": null,
              "mountpoints": [], "ro": false, "rm": false
            }
          ]
        }
      ]
    }"#;

    const MOUNTINFO: &str = "\
36 25 0:30 / / rw,relatime shared:1 - overlay overlay rw
37 36 8:1 / /mnt/data rw,relatime - ext4 /dev/sda1 rw
";

    #[derive(Default)]
    struct FakeProbe {
        lsblk: Option<Result<String, String>>,
        mountinfo: Option<Result<String, String>>,
        swaps: Option<Vec<String>>,
        usage: Mutex<HashMap<String, VolumeUsage>>,
    }

    impl FakeProbe {
        fn healthy() -> Self {
            Self {
                lsblk: Some(Ok(LSBLK.to_string())),
                mountinfo: Some(Ok(MOUNTINFO.to_string())),
                swaps: Some(vec!["/dev/sda2".to_string()]),
                usage: Mutex::new(HashMap::from([(
                    "/mnt/data".to_string(),
                    VolumeUsage {
                        total_bytes: 100,
                        used_bytes: 40,
                        available_bytes: 60,
                    },
                )])),
            }
        }
    }

    impl Probe for FakeProbe {
        fn lsblk(&self) -> Result<String, String> {
            self.lsblk
                .clone()
                .unwrap_or_else(|| Err("lsblk unavailable".to_string()))
        }

        fn mountinfo(&self) -> Result<String, String> {
            self.mountinfo
                .clone()
                .unwrap_or_else(|| Err("mountinfo unavailable".to_string()))
        }

        fn swaps(&self) -> Option<Vec<String>> {
            self.swaps.clone()
        }

        fn usage(&self, mountpoint: &str) -> Option<VolumeUsage> {
            self.usage.lock().expect("lock").get(mountpoint).copied()
        }
    }

    #[derive(Default)]
    struct FakeMountRunner {
        calls: Mutex<Vec<(String, Vec<String>)>>,
        output: Option<MountOutput>,
        spawn_error: Option<String>,
    }

    impl MountRunner for FakeMountRunner {
        fn run(&self, program: &str, args: &[String]) -> Result<MountOutput, String> {
            self.calls
                .lock()
                .expect("lock")
                .push((program.to_string(), args.to_vec()));
            if let Some(error) = &self.spawn_error {
                return Err(error.clone());
            }
            Ok(self.output.clone().unwrap_or(MountOutput {
                ok: true,
                stderr: String::new(),
            }))
        }
    }

    fn config() -> VolumesConfig {
        VolumesConfig {
            control: true,
            partition: true,
            format: true,
            mount_root: "/mnt".to_string(),
            disk_allow: vec!["/dev/sda".to_string()],
        }
    }

    fn temp_target(name: &str) -> PathBuf {
        let path = PathBuf::from("/tmp/anthracite-panel/tests")
            .join(format!("volumes-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&path).expect("target");
        path
    }

    fn mutation_config(root: &Path) -> VolumesConfig {
        VolumesConfig {
            control: true,
            partition: false,
            format: false,
            mount_root: root.to_string_lossy().into_owned(),
            disk_allow: Vec::new(),
        }
    }

    #[test]
    fn a_snapshot_links_devices_mounts_swaps_and_policy() {
        let service = VolumesService::with_probe(config(), Arc::new(FakeProbe::healthy()));
        let snapshot = service.snapshot();
        assert_eq!(snapshot.devices.len(), 3);
        assert_eq!(snapshot.devices[0].kind, VolumeKind::Disk);
        assert_eq!(snapshot.devices[1].parent.as_deref(), Some("sda"));
        assert_eq!(snapshot.mounts.len(), 2);
        assert_eq!(snapshot.swap_devices, vec!["/dev/sda2"]);
        assert_eq!(snapshot.mounts[1].usage.expect("usage").used_bytes, 40);
        assert!(snapshot.control_enabled);
        assert!(snapshot.partition_enabled);
        assert!(snapshot.format_enabled);
        assert_eq!(snapshot.mount_root, "/mnt");
        assert_eq!(snapshot.disk_allow, vec!["/dev/sda"]);
        assert!(snapshot.warning.is_none());
        assert!(snapshot.timestamp_ms > 0);
    }

    #[test]
    fn a_missing_lsblk_keeps_mounts_and_warns() {
        let mut probe = FakeProbe::healthy();
        probe.lsblk = Some(Err("lsblk is not installed".to_string()));
        let service = VolumesService::with_probe(config(), Arc::new(probe));
        let snapshot = service.snapshot();
        assert!(snapshot.devices.is_empty());
        assert_eq!(snapshot.mounts.len(), 2);
        assert!(snapshot.warning.expect("warning").contains("lsblk"));
    }

    #[test]
    fn a_missing_mountinfo_keeps_devices_and_warns() {
        let mut probe = FakeProbe::healthy();
        probe.mountinfo = Some(Err("no mountinfo".to_string()));
        let service = VolumesService::with_probe(config(), Arc::new(probe));
        let snapshot = service.snapshot();
        assert_eq!(snapshot.devices.len(), 3);
        assert!(snapshot.mounts.is_empty());
        assert!(snapshot.warning.expect("warning").contains("mount"));
    }

    #[test]
    fn unavailable_swap_is_an_unknown_not_an_empty_list() {
        let mut probe = FakeProbe::healthy();
        probe.swaps = None;
        let service = VolumesService::with_probe(config(), Arc::new(probe));
        let snapshot = service.snapshot();
        assert!(snapshot.swap_devices.is_empty());
        assert!(snapshot.warning.expect("warning").contains("swap"));
    }

    #[test]
    fn a_missing_lsblk_fstype_is_filled_from_the_mount_source() {
        let mut probe = FakeProbe::healthy();
        probe.lsblk = Some(Ok(LSBLK.replace("\"fstype\": \"ext4\"", "\"fstype\": null")));
        let service = VolumesService::with_probe(config(), Arc::new(probe));
        let snapshot = service.snapshot();
        let partition = snapshot
            .devices
            .iter()
            .find(|device| device.name == "sda1")
            .expect("sda1");
        assert_eq!(partition.fstype.as_deref(), Some("ext4"));
    }

    #[test]
    fn usage_is_best_effort_per_mountpoint() {
        let probe = FakeProbe::healthy();
        let service = VolumesService::with_probe(config(), Arc::new(probe));
        let snapshot = service.snapshot();
        assert!(snapshot.mounts[0].usage.is_none());
        assert_eq!(snapshot.mounts[1].usage.expect("usage").available_bytes, 60);
    }

    #[test]
    fn mount_validates_the_fresh_snapshot_and_runs_the_argv() {
        let target = temp_target("mount");
        let target_text = target.to_string_lossy().into_owned();
        let runner = Arc::new(FakeMountRunner::default());
        let service = VolumesService::with_parts(
            mutation_config(Path::new("/tmp")),
            Arc::new(FakeProbe::healthy()),
            runner.clone(),
            Vec::new(),
        );
        let message = service
            .mount(&VolumeMountRequest {
                device: "sda2".to_string(),
                target: target_text.clone(),
                fs_type: None,
                read_only: true,
                allow_removable: false,
            })
            .expect("mount");
        assert_eq!(
            message,
            format!("mounted /dev/sda2 on {target_text} as ext4")
        );
        assert_eq!(
            runner.calls.lock().expect("lock").as_slice(),
            &[(
                "mount".to_string(),
                vec![
                    "-o".to_string(),
                    "ro".to_string(),
                    "-t".to_string(),
                    "ext4".to_string(),
                    "/dev/sda2".to_string(),
                    target_text,
                ],
            )]
        );
        let _ = std::fs::remove_dir_all(target);
    }

    #[test]
    fn mount_refuses_unknown_devices_outside_the_mount_root_and_control_off() {
        let target = temp_target("refuse");
        let target_text = target.to_string_lossy().into_owned();
        let runner = Arc::new(FakeMountRunner::default());
        let service = VolumesService::with_parts(
            mutation_config(Path::new("/tmp")),
            Arc::new(FakeProbe::healthy()),
            runner.clone(),
            Vec::new(),
        );

        let unknown = service
            .mount(&VolumeMountRequest {
                device: "/dev/missing".to_string(),
                target: target_text.clone(),
                fs_type: Some("ext4".to_string()),
                read_only: false,
                allow_removable: false,
            })
            .expect_err("unknown");
        assert_eq!(unknown.code, "volume_not_found");

        let outside = service
            .mount(&VolumeMountRequest {
                device: "/dev/sda2".to_string(),
                target: "/etc".to_string(),
                fs_type: Some("ext4".to_string()),
                read_only: false,
                allow_removable: false,
            })
            .expect_err("outside");
        assert_eq!(outside.code, "protected_mount");
        assert!(runner.calls.lock().expect("lock").is_empty());

        let mut config = mutation_config(Path::new("/tmp"));
        config.control = false;
        let disabled = VolumesService::with_parts(
            config,
            Arc::new(FakeProbe::healthy()),
            runner.clone(),
            Vec::new(),
        );
        assert_eq!(
            disabled
                .mount(&VolumeMountRequest {
                    device: "/dev/sda2".to_string(),
                    target: target_text,
                    fs_type: None,
                    read_only: false,
                    allow_removable: false,
                })
                .expect_err("disabled")
                .code,
            "volumes_control_disabled"
        );
        let _ = std::fs::remove_dir_all(target);
    }

    #[test]
    fn a_busy_refusal_is_a_conflict() {
        let target = temp_target("busy");
        let runner = Arc::new(FakeMountRunner {
            output: Some(MountOutput {
                ok: false,
                stderr: "mount: /mnt: target is busy.".to_string(),
            }),
            ..FakeMountRunner::default()
        });
        let service = VolumesService::with_parts(
            mutation_config(Path::new("/tmp")),
            Arc::new(FakeProbe::healthy()),
            runner,
            Vec::new(),
        );
        let error = service
            .mount(&VolumeMountRequest {
                device: "/dev/sda2".to_string(),
                target: target.to_string_lossy().into_owned(),
                fs_type: None,
                read_only: false,
                allow_removable: false,
            })
            .expect_err("busy");
        assert_eq!(error.kind, ErrorKind::Conflict);
        assert_eq!(error.code, "mount_busy");
        let _ = std::fs::remove_dir_all(target);
    }

    #[test]
    fn unmount_requires_a_current_non_virtual_mountpoint_and_guards_protected_paths() {
        let target = temp_target("unmount");
        let target_text = target.to_string_lossy().into_owned();
        let mountinfo = format!(
            "36 25 0:30 / / rw,relatime shared:1 - overlay overlay rw\n\
             37 36 8:1 / {target_text} rw,relatime - ext4 /dev/sda2 rw\n"
        );
        let mut probe = FakeProbe::healthy();
        probe.mountinfo = Some(Ok(mountinfo));
        let runner = Arc::new(FakeMountRunner::default());
        let service = VolumesService::with_parts(
            mutation_config(Path::new("/tmp")),
            Arc::new(probe),
            runner.clone(),
            Vec::new(),
        );
        assert_eq!(
            service
                .unmount(&VolumeUnmountRequest {
                    target: target_text.clone()
                })
                .expect("unmount"),
            format!("unmounted {target_text}")
        );
        assert_eq!(
            runner.calls.lock().expect("lock").as_slice(),
            &[("umount".to_string(), vec![target_text.clone()])]
        );

        let root = service
            .unmount(&VolumeUnmountRequest {
                target: "/".to_string(),
            })
            .expect_err("root");
        assert_eq!(root.code, "protected_mount");

        let mut probe = FakeProbe::healthy();
        probe.mountinfo = Some(Ok(format!(
            "36 25 0:30 / / rw,relatime - overlay overlay rw\n\
             37 36 8:1 / {target_text} rw,relatime - ext4 /dev/sda2 rw\n"
        )));
        let protected = target.join("panel-data");
        let service = VolumesService::with_parts(
            mutation_config(Path::new("/tmp")),
            Arc::new(probe),
            runner,
            vec![protected],
        );
        let error = service
            .unmount(&VolumeUnmountRequest {
                target: target_text,
            })
            .expect_err("protected");
        assert_eq!(error.code, "protected_mount");
        let _ = std::fs::remove_dir_all(target);
    }
}
