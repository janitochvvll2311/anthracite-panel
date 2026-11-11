//! Deterministic block-device and mount fixture for the demo (§19 §2, `storage`).

use std::collections::BTreeMap;

use anth_contract::Error;
use serde_json::{json, Value};

use crate::text;

#[derive(serde::Serialize, serde::Deserialize)]
struct Device {
    name: String,
    path: String,
    size_bytes: u64,
    model: String,
    removable: bool,
    mounted: bool,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Mount {
    device: String,
    mountpoint: String,
    filesystem: String,
    used_bytes: u64,
    total_bytes: u64,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct StorageFixture {
    devices: Vec<Device>,
    mounts: Vec<Mount>,
    /// The files on each block device, as paths relative to its root. Mounting
    /// overlays this into the shared files tree; unmounting captures it back.
    #[serde(default)]
    device_files: BTreeMap<String, BTreeMap<String, Vec<u8>>>,
}

impl StorageFixture {
    pub fn new() -> Self {
        Self {
            devices: crate::world::devices()
                .iter()
                .map(|device| Device {
                    name: device.name.to_string(),
                    path: device.path.to_string(),
                    size_bytes: device.size_bytes,
                    model: device.model.to_string(),
                    removable: device.removable,
                    mounted: device.mounted,
                })
                .collect(),
            mounts: crate::world::mounts()
                .iter()
                .map(|mount| Mount {
                    device: mount.device.to_string(),
                    mountpoint: mount.mountpoint.to_string(),
                    filesystem: mount.filesystem.to_string(),
                    used_bytes: mount.used_bytes,
                    total_bytes: mount.total_bytes,
                })
                .collect(),
            device_files: seed_files(),
        }
    }

    pub(crate) fn mounted_devices(&self) -> Vec<(String, String)> {
        self.mounts
            .iter()
            .map(|mount| (mount.device.clone(), mount.mountpoint.clone()))
            .collect()
    }

    pub(crate) fn files_for(&self, device: &str) -> BTreeMap<String, Vec<u8>> {
        self.device_files.get(device).cloned().unwrap_or_default()
    }

    pub(crate) fn set_files(&mut self, device: &str, files: BTreeMap<String, Vec<u8>>) {
        self.device_files.insert(device.to_string(), files);
    }

    pub(crate) fn device_for(&self, mountpoint: &str) -> Option<String> {
        self.mounts
            .iter()
            .find(|mount| mount.mountpoint == mountpoint)
            .map(|mount| mount.device.clone())
    }

    pub fn handle(&mut self, function: &str, input: &Value) -> Result<Value, Error> {
        match function {
            "devices" => Ok(Value::Array(
                self.devices
                    .iter()
                    .map(|device| {
                        json!({
                            "name": device.name,
                            "path": device.path,
                            "size-bytes": device.size_bytes,
                            "model": device.model,
                            "removable": device.removable,
                            "mounted": device.mounted,
                        })
                    })
                    .collect(),
            )),
            "mounts" => Ok(Value::Array(
                self.mounts
                    .iter()
                    .map(|mount| {
                        json!({
                            "device": mount.device,
                            "mountpoint": mount.mountpoint,
                            "filesystem": mount.filesystem,
                            "used-bytes": mount.used_bytes,
                            "total-bytes": mount.total_bytes,
                        })
                    })
                    .collect(),
            )),
            "attach" => {
                let device = text(input, "device")?;
                let mountpoint = text(input, "mountpoint")?;
                let filesystem = text(input, "filesystem")?;
                if !mountpoint.starts_with('/') || mountpoint.contains("..") {
                    return Err(Error::invalid("`mountpoint` must be an absolute path"));
                }
                if filesystem.trim().is_empty() {
                    return Err(Error::invalid("`filesystem` cannot be empty"));
                }
                if self
                    .mounts
                    .iter()
                    .any(|mount| mount.mountpoint == mountpoint)
                {
                    return Err(Error::conflict(format!(
                        "`{mountpoint}` is already a mountpoint"
                    )));
                }
                let size = {
                    let found = self
                        .devices
                        .iter_mut()
                        .find(|candidate| candidate.path == device)
                        .ok_or_else(|| Error::not_found(format!("`{device}` is not a device")))?;
                    if found.mounted {
                        return Err(Error::conflict(format!("`{device}` is already mounted")));
                    }
                    found.mounted = true;
                    found.size_bytes
                };
                self.mounts.push(Mount {
                    device,
                    mountpoint,
                    filesystem,
                    used_bytes: 0,
                    total_bytes: size,
                });
                Ok(Value::Null)
            }
            "detach" => {
                let mountpoint = text(input, "mountpoint")?;
                if ["/", "/boot", "/boot/efi"].contains(&mountpoint.as_str()) {
                    return Err(Error::denied(format!("`{mountpoint}` is a system mount")));
                }
                let index = self
                    .mounts
                    .iter()
                    .position(|mount| mount.mountpoint == mountpoint)
                    .ok_or_else(|| {
                        Error::not_found(format!("`{mountpoint}` is not a mountpoint"))
                    })?;
                let removed = self.mounts.remove(index);
                if !self
                    .mounts
                    .iter()
                    .any(|mount| mount.device == removed.device)
                {
                    if let Some(device) = self
                        .devices
                        .iter_mut()
                        .find(|candidate| candidate.path == removed.device)
                    {
                        device.mounted = false;
                    }
                }
                Ok(Value::Null)
            }
            other => Err(Error::not_found(format!(
                "`anthracite:storage/storage#{other}` is not implemented"
            ))),
        }
    }
}

fn seed_files() -> BTreeMap<String, BTreeMap<String, Vec<u8>>> {
    let mut files = BTreeMap::new();
    files.insert(
        "/dev/nvme0n1p1".to_string(),
        entries(&[("EFI/BOOT/BOOTX64.EFI", "boot\n")]),
    );
    files.insert(
        "/dev/sdb1".to_string(),
        entries(&[
            ("README.txt", "Mounted demo volume.\n"),
            ("photos/beach.txt", "waves\n"),
            ("photos/forest.txt", "trees\n"),
        ]),
    );
    files.insert(
        "/dev/sdc".to_string(),
        entries(&[
            ("notes.txt", "SD card notes.\n"),
            ("DCIM/IMG_0001.txt", "photo\n"),
        ]),
    );
    files
}

fn entries(items: &[(&str, &str)]) -> BTreeMap<String, Vec<u8>> {
    items
        .iter()
        .map(|(path, text)| (path.to_string(), text.as_bytes().to_vec()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(fixture: &mut StorageFixture, function: &str, input: Value) -> Value {
        fixture
            .handle(function, &input)
            .expect("the fixture answers")
    }

    #[test]
    fn devices_and_mounts_are_stable() {
        let mut fixture = StorageFixture::new();
        let devices = call(&mut fixture, "devices", Value::Null);
        assert_eq!(devices[0]["name"], "nvme0n1p1");
        assert_eq!(devices[3]["mounted"], false);
        let mounts = call(&mut fixture, "mounts", Value::Null);
        assert_eq!(mounts[1]["mountpoint"], "/");
        assert!(
            mounts[1]["used-bytes"].as_u64().unwrap() < mounts[1]["total-bytes"].as_u64().unwrap()
        );
    }

    #[test]
    fn a_device_can_be_mounted_and_unmounted() {
        let mut fixture = StorageFixture::new();
        call(
            &mut fixture,
            "attach",
            json!({"device": "/dev/sdc", "mountpoint": "/media/sd", "filesystem": "exfat"}),
        );
        let mounts = call(&mut fixture, "mounts", Value::Null);
        assert!(mounts
            .as_array()
            .expect("mounts")
            .iter()
            .any(|mount| mount["mountpoint"] == "/media/sd"));
        let devices = call(&mut fixture, "devices", Value::Null);
        assert_eq!(devices[3]["mounted"], true);

        call(&mut fixture, "detach", json!({"mountpoint": "/media/sd"}));
        let mounts = call(&mut fixture, "mounts", Value::Null);
        assert!(!mounts
            .as_array()
            .expect("mounts")
            .iter()
            .any(|mount| mount["mountpoint"] == "/media/sd"));
        let error = fixture
            .handle(
                "attach",
                &json!({"device": "/dev/nope", "mountpoint": "/x", "filesystem": "ext4"}),
            )
            .expect_err("missing device");
        assert_eq!(error.kind(), anth_contract::ErrorKind::NotFound);
        let error = fixture
            .handle("detach", &json!({"mountpoint": "/nope"}))
            .expect_err("missing mountpoint");
        assert_eq!(error.kind(), anth_contract::ErrorKind::NotFound);
    }
}
