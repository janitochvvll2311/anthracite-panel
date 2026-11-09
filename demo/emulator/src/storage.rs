//! Deterministic block-device and mount fixture for the demo (§19 §2, `storage`).

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
}

impl StorageFixture {
    pub fn new() -> Self {
        Self {
            devices: vec![
                Device {
                    name: "nvme0n1p1".to_string(),
                    path: "/dev/nvme0n1p1".to_string(),
                    size_bytes: 536_870_912,
                    model: "Demo NVMe".to_string(),
                    removable: false,
                    mounted: true,
                },
                Device {
                    name: "nvme0n1p2".to_string(),
                    path: "/dev/nvme0n1p2".to_string(),
                    size_bytes: 51_539_607_552,
                    model: "Demo NVMe".to_string(),
                    removable: false,
                    mounted: true,
                },
                Device {
                    name: "sdb1".to_string(),
                    path: "/dev/sdb1".to_string(),
                    size_bytes: 68_719_476_736,
                    model: "Demo USB".to_string(),
                    removable: true,
                    mounted: true,
                },
                Device {
                    name: "sdc".to_string(),
                    path: "/dev/sdc".to_string(),
                    size_bytes: 15_931_064_320,
                    model: "Demo SD".to_string(),
                    removable: true,
                    mounted: false,
                },
            ],
            mounts: vec![
                Mount {
                    device: "/dev/nvme0n1p1".to_string(),
                    mountpoint: "/boot/efi".to_string(),
                    filesystem: "vfat".to_string(),
                    used_bytes: 67_108_864,
                    total_bytes: 536_870_912,
                },
                Mount {
                    device: "/dev/nvme0n1p2".to_string(),
                    mountpoint: "/".to_string(),
                    filesystem: "ext4".to_string(),
                    used_bytes: 19_327_352_832,
                    total_bytes: 51_539_607_552,
                },
                Mount {
                    device: "/dev/sdb1".to_string(),
                    mountpoint: "/media/demo".to_string(),
                    filesystem: "exfat".to_string(),
                    used_bytes: 12_884_901_888,
                    total_bytes: 68_719_476_736,
                },
            ],
        }
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
                let device = text(input, "device")?;
                let index = self
                    .mounts
                    .iter()
                    .position(|mount| mount.device == device)
                    .ok_or_else(|| Error::not_found(format!("`{device}` is not mounted")))?;
                self.mounts.remove(index);
                if let Some(device) = self
                    .devices
                    .iter_mut()
                    .find(|candidate| candidate.path == device)
                {
                    device.mounted = false;
                }
                Ok(Value::Null)
            }
            other => Err(Error::not_found(format!(
                "`anthracite:storage/storage#{other}` is not implemented"
            ))),
        }
    }
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

        call(&mut fixture, "detach", json!({"device": "/dev/sdc"}));
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
    }
}
