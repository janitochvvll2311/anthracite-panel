//! Deterministic block-device and mount fixture for the demo (§19 §2, `storage`).

use anth_contract::Error;
use serde_json::{json, Value};

pub struct StorageFixture;

impl StorageFixture {
    pub fn new() -> Self {
        Self
    }

    pub fn handle(&self, function: &str, _input: &Value) -> Result<Value, Error> {
        match function {
            "devices" => Ok(json!([
                {
                    "name": "nvme0n1p1",
                    "path": "/dev/nvme0n1p1",
                    "size-bytes": 536_870_912u64,
                    "model": "Demo NVMe",
                    "removable": false,
                    "mounted": true,
                },
                {
                    "name": "nvme0n1p2",
                    "path": "/dev/nvme0n1p2",
                    "size-bytes": 51_539_607_552u64,
                    "model": "Demo NVMe",
                    "removable": false,
                    "mounted": true,
                },
                {
                    "name": "sdb1",
                    "path": "/dev/sdb1",
                    "size-bytes": 68_719_476_736u64,
                    "model": "Demo USB",
                    "removable": true,
                    "mounted": true,
                },
                {
                    "name": "sdc",
                    "path": "/dev/sdc",
                    "size-bytes": 15_931_064_320u64,
                    "model": "Demo SD",
                    "removable": true,
                    "mounted": false,
                },
            ])),
            "mounts" => Ok(json!([
                {
                    "device": "/dev/nvme0n1p1",
                    "mountpoint": "/boot/efi",
                    "filesystem": "vfat",
                    "used-bytes": 67_108_864u64,
                    "total-bytes": 536_870_912u64,
                },
                {
                    "device": "/dev/nvme0n1p2",
                    "mountpoint": "/",
                    "filesystem": "ext4",
                    "used-bytes": 19_327_352_832u64,
                    "total-bytes": 51_539_607_552u64,
                },
                {
                    "device": "/dev/sdb1",
                    "mountpoint": "/media/demo",
                    "filesystem": "exfat",
                    "used-bytes": 12_884_901_888u64,
                    "total-bytes": 68_719_476_736u64,
                },
            ])),
            other => Err(Error::not_found(format!(
                "`anthracite:storage/storage#{other}` is not implemented"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn devices_and_mounts_are_stable() {
        let fixture = StorageFixture::new();
        let devices = fixture.handle("devices", &Value::Null).expect("devices");
        assert_eq!(devices[0]["name"], "nvme0n1p1");
        assert_eq!(devices[3]["mounted"], false);
        let mounts = fixture.handle("mounts", &Value::Null).expect("mounts");
        assert_eq!(mounts[1]["mountpoint"], "/");
        assert!(
            mounts[1]["used-bytes"].as_u64().unwrap() < mounts[1]["total-bytes"].as_u64().unwrap()
        );
    }
}
