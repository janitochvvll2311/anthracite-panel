//! Storage volume inventory and mount requests.
//!
//! These types are the transport contract only. The daemon owns the `lsblk`
//! and `mountinfo` reads, the mount policy and the guards. Devices and mounts
//! are separate lists on purpose: `lsblk` cannot see overlay or tmpfs, and
//! mountinfo knows nothing about a device that is present but unmounted.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum VolumeKind {
    Disk,
    Part,
    /// A device-mapper target, including LUKS (`crypt`) and LVM (`lvm`).
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
        let value = value.to_ascii_lowercase();
        if value.starts_with("raid") {
            return Ok(Self::Raid);
        }
        match value.as_str() {
            "disk" => Ok(Self::Disk),
            "part" => Ok(Self::Part),
            "crypt" => Ok(Self::Crypt),
            "lvm" => Ok(Self::Lvm),
            "raid" => Ok(Self::Raid),
            "loop" => Ok(Self::Loop),
            "rom" => Ok(Self::Rom),
            // `lsblk` gains types over time (`zoned`, `mpath`, …). An unknown
            // one is still a device, so it degrades instead of failing.
            _ => Ok(Self::Other),
        }
    }
}

impl fmt::Display for VolumeKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// One block device as reported by `lsblk`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct VolumeDevice {
    /// Kernel name: `sda1`, `dm-0`, `nvme0n1p2`. Stable across reboots.
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
    /// Every mountpoint currently using this device, in kernel order. Always
    /// serialized, empty for an unmounted device: clients read it as a list
    /// without checking for the key first.
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
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VolumeUsage {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
}

/// One entry from `/proc/self/mountinfo`, restricted to the fields the panel
/// uses.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct VolumeMount {
    pub mountpoint: String,
    /// Device path, filesystem name or network share.
    pub source: String,
    pub fstype: String,
    /// Comma-separated mount options as the kernel reports them.
    pub options: String,
    pub read_only: bool,
    /// Kernel or pseudo filesystems (`proc`, `tmpfs`, `overlay`, …). These are
    /// never unmountable through the panel.
    pub virtual_fs: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<VolumeUsage>,
}

/// One point-in-time view of the storage attached to the host.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct VolumeSnapshot {
    pub devices: Vec<VolumeDevice>,
    pub mounts: Vec<VolumeMount>,
    /// Whether the daemon accepts mount/unmount requests.
    pub control_enabled: bool,
    /// Wall-clock time of the snapshot in milliseconds since the Unix epoch.
    pub timestamp_ms: u64,
    /// Number of devices found before any truncation.
    pub total: usize,
    /// Whether `devices` was shortened to fit the protocol line cap.
    pub truncated: bool,
    /// Human-readable notice when the snapshot was shortened or degraded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VolumeErrorKind {
    /// `[volumes] control` is off.
    Disabled,
    /// The target is `/`, a virtual filesystem or an ancestor of the panel's
    /// own paths.
    Protected,
    NotFound,
    /// The filesystem is in use (`EBUSY`).
    Busy,
    PermissionDenied,
    Invalid,
    Failed,
}

impl VolumeErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Protected => "protected",
            Self::NotFound => "not_found",
            Self::Busy => "busy",
            Self::PermissionDenied => "permission_denied",
            Self::Invalid => "invalid",
            Self::Failed => "failed",
        }
    }
}

impl fmt::Display for VolumeErrorKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct VolumeError {
    pub kind: VolumeErrorKind,
    pub detail: String,
}

impl VolumeError {
    pub fn new(kind: VolumeErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }
}

impl fmt::Display for VolumeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.kind, self.detail)
    }
}

impl std::error::Error for VolumeError {}
