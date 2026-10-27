//! Storage inventory: block devices, mounts and swap areas.
//!
//! The read path is deliberately read-only. Mount, partition and format
//! operations arrive in later slices and validate against a fresh snapshot.

mod parse;

use std::ffi::CString;
use std::process::Command;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anth_wire::{VolumeSnapshot, VolumeUsage};

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
}

impl VolumesService {
    pub fn new(config: VolumesConfig) -> Self {
        Self::with_probe(config, Arc::new(SystemProbe))
    }

    /// Replaces the host probe. Used by tests.
    pub fn with_probe(config: VolumesConfig, probe: Arc<dyn Probe>) -> Self {
        Self { config, probe }
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

    fn config() -> VolumesConfig {
        VolumesConfig {
            control: true,
            partition: true,
            format: true,
            mount_root: "/mnt".to_string(),
            disk_allow: vec!["/dev/sda".to_string()],
        }
    }

    #[test]
    fn a_snapshot_links_devices_mounts_swaps_and_policy() {
        let service = VolumesService::with_probe(config(), Arc::new(FakeProbe::healthy()));
        let snapshot = service.snapshot();
        assert_eq!(snapshot.devices.len(), 2);
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
        assert_eq!(snapshot.devices.len(), 2);
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
}
