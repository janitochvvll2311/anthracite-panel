//! Storage inventory values.
//!
//! Two sources answer two different questions. `lsblk` knows the block-device
//! tree, filesystems, labels and UUIDs but cannot see an overlay or a tmpfs.
//! `/proc/self/mountinfo` knows every mount but nothing about a device that is
//! present and unmounted. A snapshot carries both and links them by source path.
//!
//! This slice is read-only. Mount, partition and format operations are separate
//! requests gated by `[volumes]`.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// The kind of block device `lsblk` reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VolumeKind {
    Disk,
    Part,
    /// Device-mapper target, including LUKS (`crypt`) and LVM (`lvm`).
    Crypt,
    Lvm,
    Raid,
    Loop,
    /// Optical or otherwise read-only removable media.
    Rom,
    #[default]
    Other,
}

impl VolumeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disk => "disk",
            Self::Part => "part",
            Self::Crypt => "crypt",
            Self::Lvm => "lvm",
            Self::Raid => "raid",
            Self::Loop => "loop",
            Self::Rom => "rom",
            Self::Other => "other",
        }
    }
}

impl FromStr for VolumeKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let value = value.trim().to_ascii_lowercase();
        if value.starts_with("raid") {
            return Ok(Self::Raid);
        }
        Ok(match value.as_str() {
            "disk" => Self::Disk,
            "part" => Self::Part,
            "crypt" => Self::Crypt,
            "lvm" => Self::Lvm,
            "raid" => Self::Raid,
            "loop" => Self::Loop,
            "rom" => Self::Rom,
            _ => Self::Other,
        })
    }
}

impl fmt::Display for VolumeKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// One block device as `lsblk` reports it, flattened with a parent link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VolumeDevice {
    /// Kernel name: `sda1`, `dm-0`, `nvme0n1p2`.
    pub name: String,
    /// Device path: `/dev/sda1`.
    pub path: String,
    pub kind: VolumeKind,
    /// Parent device name, so the client can rebuild the tree. `None` for disks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    pub size_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fstype: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uuid: Option<String>,
    /// Every mountpoint currently using this device, in kernel order.
    #[serde(default)]
    pub mountpoints: Vec<String>,
    pub read_only: bool,
    pub removable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport: Option<String>,
}

impl VolumeDevice {
    pub fn is_mounted(&self) -> bool {
        !self.mountpoints.is_empty()
    }
}

/// Free-space counters for one mountpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct VolumeUsage {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
}

/// One entry from `/proc/self/mountinfo`, restricted to the fields the panel
/// uses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VolumeMount {
    pub mountpoint: String,
    /// Device path, filesystem name or network share.
    pub source: String,
    pub fstype: String,
    /// Comma-separated mount options as the kernel reports them.
    pub options: String,
    pub read_only: bool,
    /// Kernel or pseudo filesystems (`proc`, `tmpfs`, `overlay`, ...). These are
    /// reported but never valid mount or unmount targets.
    pub virtual_fs: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<VolumeUsage>,
}

/// A request to mount one device under the configured mount root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VolumeMountRequest {
    /// Device path or kernel name.
    pub device: String,
    /// Absolute target directory, under `[volumes] mount_root`.
    pub target: String,
    /// Filesystem type; defaults to the type `lsblk` detected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fs_type: Option<String>,
    #[serde(default)]
    pub read_only: bool,
    /// Allow a removable device; refused by default.
    #[serde(default)]
    pub allow_removable: bool,
}

/// A request to unmount one current mountpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VolumeUnmountRequest {
    pub target: String,
}

/// One point-in-time view of the storage attached to the host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VolumeSnapshot {
    pub devices: Vec<VolumeDevice>,
    pub mounts: Vec<VolumeMount>,
    /// Devices backing an active swap area. Swap is never in mountinfo, so
    /// this is the only way a guard can see it.
    #[serde(default)]
    pub swap_devices: Vec<String>,
    /// Whether `[volumes] control` allows mount/unmount.
    pub control_enabled: bool,
    /// Whether `[volumes] partition` allows partition-table edits.
    pub partition_enabled: bool,
    /// Whether `[volumes] format` allows mkfs.
    pub format_enabled: bool,
    /// The directory new mounts must stay under.
    pub mount_root: String,
    /// The disks partition and format operations may touch.
    #[serde(default)]
    pub disk_allow: Vec<String>,
    /// A degraded read, for example because `lsblk` is unavailable. The other
    /// source is still reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    /// When the snapshot was taken, in milliseconds since the Unix epoch.
    pub timestamp_ms: u64,
}
