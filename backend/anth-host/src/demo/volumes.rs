//! Synthetic storage inventory and mutations for the demo build.
//!
//! Nothing here calls `lsblk`, mounts anything or writes a partition table.
//! The table lives in memory, so the UI exercises the same wire types and the
//! same guards without touching the host.

use std::sync::Mutex;

use anth_wire::{
    ErrorKind, VolumeDevice, VolumeFormatRequest, VolumeKind, VolumeMount, VolumeMountRequest,
    VolumePartitionCreate, VolumePartitionDelete, VolumePartitionResize, VolumeSnapshot,
    VolumeUnmountRequest, VolumeUsage, WireError,
};

use crate::backend::VolumesService;
use crate::now_ms;
use crate::volumes::VolumesSettings;

pub struct DemoVolumes {
    settings: VolumesSettings,
    state: Mutex<DemoState>,
}

struct DemoState {
    devices: Vec<VolumeDevice>,
    mounts: Vec<VolumeMount>,
    swap_devices: Vec<String>,
}

impl DemoVolumes {
    pub fn new(settings: VolumesSettings) -> Self {
        let mut settings = settings;
        // The demo ships a spare disk so partition and format can be tried
        // without asking the operator to allow a real device first.
        if settings.disk_allow.is_empty() {
            settings.disk_allow = vec!["/dev/sdb".to_string()];
        }
        Self {
            settings,
            state: Mutex::new(seed()),
        }
    }

    fn find_device<'a>(state: &'a DemoState, device: &str) -> Option<&'a VolumeDevice> {
        state
            .devices
            .iter()
            .find(|entry| entry.path == device || entry.name == device)
    }

    fn allowed(&self, device: &VolumeDevice) -> bool {
        if self.settings.disk_allow.is_empty() {
            return false;
        }
        self.settings.disk_allow.iter().any(|allowed| {
            allowed == &device.path
                || allowed == &device.name
                || device
                    .parent
                    .as_deref()
                    .is_some_and(|parent| allowed == parent)
        })
    }

    fn view(&self, state: &DemoState) -> VolumeSnapshot {
        VolumeSnapshot {
            devices: state.devices.clone(),
            mounts: state.mounts.clone(),
            swap_devices: state.swap_devices.clone(),
            control_enabled: self.settings.control,
            partition_enabled: self.settings.partition,
            format_enabled: self.settings.format,
            mount_root: self.settings.mount_root.clone(),
            disk_allow: self.settings.disk_allow.clone(),
            warning: None,
            timestamp_ms: now_ms(),
        }
    }
}

impl VolumesService for DemoVolumes {
    fn snapshot(&self) -> VolumeSnapshot {
        let state = self.state.lock().expect("demo volumes lock");
        self.view(&state)
    }

    fn mount(&self, request: &VolumeMountRequest) -> Result<String, WireError> {
        if !self.settings.control {
            return Err(WireError::new(
                ErrorKind::Forbidden,
                "volume_control_disabled",
                "storage control is disabled; set `[volumes] control = true`",
            ));
        }
        let root = self.settings.mount_root.trim_end_matches('/');
        if !request.target.starts_with(&format!("{root}/")) {
            return Err(WireError::new(
                ErrorKind::Invalid,
                "mount_target_outside_root",
                format!("`{}` is not under `{root}`", request.target),
            ));
        }
        let mut state = self.state.lock().expect("demo volumes lock");
        let device = Self::find_device(&state, &request.device)
            .cloned()
            .ok_or_else(|| {
                WireError::new(
                    ErrorKind::NotFound,
                    "device_not_found",
                    format!("no device `{}`", request.device),
                )
            })?;
        if device
            .mountpoints
            .iter()
            .any(|point| point == &request.target)
        {
            return Err(WireError::new(
                ErrorKind::Conflict,
                "mount_target_busy",
                format!("`{}` is already a mountpoint", request.target),
            ));
        }
        let fstype = request
            .fs_type
            .clone()
            .or_else(|| device.fstype.clone())
            .ok_or_else(|| {
                WireError::new(
                    ErrorKind::Invalid,
                    "filesystem_unknown",
                    "the device has no filesystem; pass `fs_type`",
                )
            })?;
        if let Some(entry) = state
            .devices
            .iter_mut()
            .find(|entry| entry.path == device.path)
        {
            entry.mountpoints.push(request.target.clone());
        }
        state.mounts.push(VolumeMount {
            mountpoint: request.target.clone(),
            source: device.path.clone(),
            fstype,
            options: if request.read_only {
                "ro,relatime".to_string()
            } else {
                "rw,relatime".to_string()
            },
            read_only: request.read_only,
            virtual_fs: false,
            usage: Some(VolumeUsage {
                total_bytes: device.size_bytes,
                used_bytes: device.size_bytes / 4,
                available_bytes: device.size_bytes - device.size_bytes / 4,
            }),
        });
        Ok(format!("mounted {} at {}", device.path, request.target))
    }

    fn unmount(&self, request: &VolumeUnmountRequest) -> Result<String, WireError> {
        if !self.settings.control {
            return Err(WireError::new(
                ErrorKind::Forbidden,
                "volume_control_disabled",
                "storage control is disabled; set `[volumes] control = true`",
            ));
        }
        let mut state = self.state.lock().expect("demo volumes lock");
        let index = state
            .mounts
            .iter()
            .position(|mount| mount.mountpoint == request.target)
            .ok_or_else(|| {
                WireError::new(
                    ErrorKind::NotFound,
                    "mount_not_found",
                    format!("`{}` is not a mountpoint", request.target),
                )
            })?;
        let mount = state.mounts.remove(index);
        if let Some(device) = state
            .devices
            .iter_mut()
            .find(|entry| entry.path == mount.source)
        {
            device.mountpoints.retain(|point| point != &request.target);
        }
        Ok(format!("unmounted {}", request.target))
    }

    fn create_partition(&self, request: &VolumePartitionCreate) -> Result<String, WireError> {
        if !self.settings.partition {
            return Err(WireError::new(
                ErrorKind::Forbidden,
                "partition_disabled",
                "partition edits are disabled; set `[volumes] partition = true`",
            ));
        }
        if request.size_bytes == 0 {
            return Err(WireError::new(
                ErrorKind::Invalid,
                "partition_size_invalid",
                "a partition needs a size",
            ));
        }
        let mut state = self.state.lock().expect("demo volumes lock");
        let disk = Self::find_device(&state, &request.disk)
            .cloned()
            .ok_or_else(|| {
                WireError::new(
                    ErrorKind::NotFound,
                    "device_not_found",
                    format!("no disk `{}`", request.disk),
                )
            })?;
        if disk.kind != VolumeKind::Disk {
            return Err(WireError::new(
                ErrorKind::Invalid,
                "not_a_disk",
                format!("`{}` is not a whole disk", disk.path),
            ));
        }
        if !self.allowed(&disk) {
            return Err(WireError::new(
                ErrorKind::Forbidden,
                "disk_not_allowed",
                format!("`{}` is not in `[volumes] disk_allow`", disk.path),
            ));
        }
        let number = state
            .devices
            .iter()
            .filter(|entry| entry.parent.as_deref() == Some(disk.name.as_str()))
            .count() as u32
            + 1;
        let name = format!("{}{number}", disk.name);
        let path = format!("{}{number}", disk.path);
        let fstype = match request.kind {
            anth_wire::PartitionKind::Linux => "ext4",
            anth_wire::PartitionKind::Swap => "swap",
            anth_wire::PartitionKind::Efi => "vfat",
        };
        state.devices.push(VolumeDevice {
            name: name.clone(),
            path: path.clone(),
            kind: VolumeKind::Part,
            parent: Some(disk.name),
            size_bytes: request.size_bytes,
            fstype: Some(fstype.to_string()),
            label: None,
            uuid: Some(format!("demo-{name}")),
            mountpoints: Vec::new(),
            read_only: false,
            removable: false,
            model: None,
            transport: Some("demo".to_string()),
        });
        Ok(format!("created {path}"))
    }

    fn resize_partition(&self, request: &VolumePartitionResize) -> Result<String, WireError> {
        if !self.settings.partition {
            return Err(WireError::new(
                ErrorKind::Forbidden,
                "partition_disabled",
                "partition edits are disabled; set `[volumes] partition = true`",
            ));
        }
        let mut state = self.state.lock().expect("demo volumes lock");
        let disk = Self::find_device(&state, &request.disk)
            .cloned()
            .ok_or_else(|| {
                WireError::new(
                    ErrorKind::NotFound,
                    "device_not_found",
                    format!("no disk `{}`", request.disk),
                )
            })?;
        if !self.allowed(&disk) {
            return Err(WireError::new(
                ErrorKind::Forbidden,
                "disk_not_allowed",
                format!("`{}` is not in `[volumes] disk_allow`", disk.path),
            ));
        }
        let partition = state
            .devices
            .iter_mut()
            .find(|entry| {
                entry.parent.as_deref() == Some(disk.name.as_str())
                    && partition_number(&entry.name) == Some(request.number)
            })
            .ok_or_else(|| {
                WireError::new(
                    ErrorKind::NotFound,
                    "partition_not_found",
                    format!("`{}` has no partition {}", disk.path, request.number),
                )
            })?;
        partition.size_bytes = request.size_bytes;
        Ok(format!(
            "resized {} to {} bytes",
            partition.path, request.size_bytes
        ))
    }

    fn delete_partition(&self, request: &VolumePartitionDelete) -> Result<String, WireError> {
        if !self.settings.partition {
            return Err(WireError::new(
                ErrorKind::Forbidden,
                "partition_disabled",
                "partition edits are disabled; set `[volumes] partition = true`",
            ));
        }
        let mut state = self.state.lock().expect("demo volumes lock");
        let disk = Self::find_device(&state, &request.disk)
            .cloned()
            .ok_or_else(|| {
                WireError::new(
                    ErrorKind::NotFound,
                    "device_not_found",
                    format!("no disk `{}`", request.disk),
                )
            })?;
        if !self.allowed(&disk) {
            return Err(WireError::new(
                ErrorKind::Forbidden,
                "disk_not_allowed",
                format!("`{}` is not in `[volumes] disk_allow`", disk.path),
            ));
        }
        let index = state
            .devices
            .iter()
            .position(|entry| {
                entry.parent.as_deref() == Some(disk.name.as_str())
                    && partition_number(&entry.name) == Some(request.number)
            })
            .ok_or_else(|| {
                WireError::new(
                    ErrorKind::NotFound,
                    "partition_not_found",
                    format!("`{}` has no partition {}", disk.path, request.number),
                )
            })?;
        let removed = state.devices.remove(index);
        Ok(format!("deleted {}", removed.path))
    }

    fn format_partition(&self, request: &VolumeFormatRequest) -> Result<String, WireError> {
        if !self.settings.format {
            return Err(WireError::new(
                ErrorKind::Forbidden,
                "format_disabled",
                "formatting is disabled; set `[volumes] format = true`",
            ));
        }
        let mut state = self.state.lock().expect("demo volumes lock");
        let device = Self::find_device(&state, &request.partition)
            .cloned()
            .ok_or_else(|| {
                WireError::new(
                    ErrorKind::NotFound,
                    "device_not_found",
                    format!("no device `{}`", request.partition),
                )
            })?;
        if device.kind == VolumeKind::Disk {
            return Err(WireError::new(
                ErrorKind::Invalid,
                "whole_disk_format",
                "format a partition, not the whole disk",
            ));
        }
        if let Some(entry) = state
            .devices
            .iter_mut()
            .find(|entry| entry.path == device.path)
        {
            entry.fstype = Some(request.fs_type.clone());
            entry.label = request.label.clone();
        }
        Ok(format!("formatted {} as {}", device.path, request.fs_type))
    }
}

fn partition_number(name: &str) -> Option<u32> {
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

fn seed() -> DemoState {
    let device = |name: &str,
                  kind: VolumeKind,
                  parent: Option<&str>,
                  size_bytes: u64,
                  fstype: Option<&str>,
                  label: Option<&str>,
                  mountpoints: &[&str]| VolumeDevice {
        name: name.to_string(),
        path: format!("/dev/{name}"),
        kind,
        parent: parent.map(str::to_string),
        size_bytes,
        fstype: fstype.map(str::to_string),
        label: label.map(str::to_string),
        uuid: Some(format!("demo-{name}")),
        mountpoints: mountpoints.iter().map(|point| point.to_string()).collect(),
        read_only: false,
        removable: false,
        model: Some("Demo Disk".to_string()),
        transport: Some("demo".to_string()),
    };
    let usage = |total_bytes: u64| VolumeUsage {
        total_bytes,
        used_bytes: total_bytes / 4,
        available_bytes: total_bytes - total_bytes / 4,
    };
    DemoState {
        devices: vec![
            device(
                "sda",
                VolumeKind::Disk,
                None,
                64 * 1024 * 1024 * 1024,
                None,
                None,
                &[],
            ),
            device(
                "sda1",
                VolumeKind::Part,
                Some("sda"),
                32 * 1024 * 1024 * 1024,
                Some("ext4"),
                Some("demo-root"),
                &["/"],
            ),
            device(
                "sdb",
                VolumeKind::Disk,
                None,
                128 * 1024 * 1024 * 1024,
                None,
                None,
                &[],
            ),
            device(
                "sdb1",
                VolumeKind::Part,
                Some("sdb"),
                128 * 1024 * 1024 * 1024,
                Some("xfs"),
                Some("demo-data"),
                &["/mnt/data"],
            ),
        ],
        mounts: vec![
            VolumeMount {
                mountpoint: "/".to_string(),
                source: "/dev/sda1".to_string(),
                fstype: "ext4".to_string(),
                options: "rw,relatime".to_string(),
                read_only: false,
                virtual_fs: false,
                usage: Some(usage(32 * 1024 * 1024 * 1024)),
            },
            VolumeMount {
                mountpoint: "/mnt/data".to_string(),
                source: "/dev/sdb1".to_string(),
                fstype: "xfs".to_string(),
                options: "rw,relatime".to_string(),
                read_only: false,
                virtual_fs: false,
                usage: Some(usage(128 * 1024 * 1024 * 1024)),
            },
            VolumeMount {
                mountpoint: "/proc".to_string(),
                source: "proc".to_string(),
                fstype: "proc".to_string(),
                options: "rw,nosuid,nodev,noexec,relatime".to_string(),
                read_only: false,
                virtual_fs: true,
                usage: None,
            },
        ],
        swap_devices: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service() -> DemoVolumes {
        DemoVolumes::new(VolumesSettings {
            control: true,
            partition: true,
            format: true,
            mount_root: "/mnt".to_string(),
            disk_allow: Vec::new(),
        })
    }

    #[test]
    fn mutations_update_the_in_memory_table() {
        let volumes = service();
        let created = volumes
            .create_partition(&VolumePartitionCreate {
                disk: "/dev/sdb".to_string(),
                kind: anth_wire::PartitionKind::Linux,
                start_bytes: 0,
                size_bytes: 2_147_483_648,
            })
            .expect("create");
        assert!(created.contains("/dev/sdb2"));

        volumes
            .format_partition(&VolumeFormatRequest {
                partition: "/dev/sdb2".to_string(),
                fs_type: "ext4".to_string(),
                label: Some("demo".to_string()),
            })
            .expect("format");
        volumes
            .resize_partition(&VolumePartitionResize {
                disk: "/dev/sdb".to_string(),
                number: 2,
                size_bytes: 4_294_967_296,
            })
            .expect("resize");

        let snapshot = volumes.snapshot();
        let partition = snapshot
            .devices
            .iter()
            .find(|device| device.name == "sdb2")
            .expect("partition");
        assert_eq!(partition.size_bytes, 4_294_967_296);
        assert_eq!(partition.fstype.as_deref(), Some("ext4"));
        assert_eq!(partition.label.as_deref(), Some("demo"));

        volumes
            .delete_partition(&VolumePartitionDelete {
                disk: "/dev/sdb".to_string(),
                number: 2,
            })
            .expect("delete");
        assert!(
            !volumes
                .snapshot()
                .devices
                .iter()
                .any(|device| device.name == "sdb2")
        );
    }

    #[test]
    fn mount_and_unmount_stay_under_the_mount_root() {
        let volumes = service();
        let outside = volumes.mount(&VolumeMountRequest {
            device: "/dev/sdb1".to_string(),
            target: "/tmp/demo".to_string(),
            fs_type: None,
            read_only: false,
            allow_removable: false,
        });
        assert!(outside.is_err());

        volumes
            .mount(&VolumeMountRequest {
                device: "/dev/sda1".to_string(),
                target: "/mnt/demo".to_string(),
                fs_type: None,
                read_only: true,
                allow_removable: false,
            })
            .expect("mount");
        assert!(
            volumes
                .snapshot()
                .mounts
                .iter()
                .any(|mount| mount.mountpoint == "/mnt/demo")
        );
        volumes
            .unmount(&VolumeUnmountRequest {
                target: "/mnt/demo".to_string(),
            })
            .expect("unmount");
        assert!(
            !volumes
                .snapshot()
                .mounts
                .iter()
                .any(|mount| mount.mountpoint == "/mnt/demo")
        );
    }
}
