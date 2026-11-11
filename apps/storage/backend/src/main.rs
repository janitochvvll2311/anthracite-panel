//! Host storage status for the Storage app.
//!
//! The read half lists block devices from `/sys/block` and the mounts from
//! `/proc/self/mounts`, with usage from `statvfs(3)`. Attaching and detaching
//! need `CAP_SYS_ADMIN`; that approved apply flow is the next slice, so those
//! functions answer with a typed refusal for now.

use std::collections::BTreeMap;
use std::ffi::CString;

use anth_contract::Error;
use anth_service::Handler;
use serde_json::{json, Value};

const STORAGE: &str = "anthracite:storage/storage@1.1.0";

struct Service;

impl Handler for Service {
    fn call(&self, interface: &str, function: &str, _input: Value) -> Result<Value, Error> {
        if interface != STORAGE {
            return Err(Error::not_found(format!(
                "`{interface}` is not exported by the storage service"
            )));
        }
        match function {
            "devices" => Ok(Value::Array(devices())),
            "mounts" => Ok(Value::Array(mounts())),
            "attach" | "detach" => Err(Error::denied(
                "mounting and unmounting need the approved storage apply flow",
            )),
            other => Err(Error::not_found(format!(
                "`anthracite:storage/storage#{other}` is not implemented"
            ))),
        }
    }
}

#[tokio::main]
async fn main() {
    let socket = match anth_service::socket_argument() {
        Ok(socket) => socket,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    if let Err(error) = anth_service::serve(&socket, std::sync::Arc::new(Service)).await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

struct MountEntry {
    device: String,
    mountpoint: String,
    filesystem: String,
    used_bytes: u64,
    total_bytes: u64,
}

fn read(path: &str) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

fn read_trim(path: &str) -> String {
    read(path).trim().to_string()
}

fn mounts() -> Vec<Value> {
    mount_entries()
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
        .collect()
}

fn mount_entries() -> Vec<MountEntry> {
    let mut entries = Vec::new();
    for line in read("/proc/self/mounts").lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 3 {
            continue;
        }
        let device = unescape(fields[0]);
        let mountpoint = unescape(fields[1]);
        let filesystem = fields[2].to_string();
        if !is_storage_mount(&device, &mountpoint, &filesystem) {
            continue;
        }
        let (used_bytes, total_bytes) = usage(&mountpoint).unwrap_or((0, 0));
        entries.push(MountEntry {
            device,
            mountpoint,
            filesystem,
            used_bytes,
            total_bytes,
        });
    }
    entries
}

/// `/proc/self/mounts` escapes spaces and backslashes in the first two fields.
fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' && index + 3 < bytes.len() {
            if let Ok(octal) = u8::from_str_radix(&value[index + 1..index + 4], 8) {
                out.push(octal as char);
                index += 4;
                continue;
            }
        }
        out.push(bytes[index] as char);
        index += 1;
    }
    out
}

fn is_storage_mount(device: &str, mountpoint: &str, filesystem: &str) -> bool {
    if pseudo(filesystem) || pseudo_mount(mountpoint) {
        return false;
    }
    if std::fs::metadata(mountpoint)
        .map(|meta| meta.is_file())
        .unwrap_or(false)
    {
        return false;
    }
    mountpoint == "/" || device.starts_with("/dev/")
}

fn pseudo(filesystem: &str) -> bool {
    matches!(
        filesystem,
        "proc"
            | "sysfs"
            | "tmpfs"
            | "devtmpfs"
            | "devpts"
            | "cgroup"
            | "cgroup2"
            | "securityfs"
            | "debugfs"
            | "tracefs"
            | "pstore"
            | "bpf"
            | "configfs"
            | "fusectl"
            | "mqueue"
            | "hugetlbfs"
            | "autofs"
            | "nsfs"
            | "efivarfs"
            | "binfmt_misc"
            | "ramfs"
            | "squashfs"
    )
}

fn pseudo_mount(mountpoint: &str) -> bool {
    ["/proc", "/sys", "/dev", "/run"]
        .iter()
        .any(|prefix| mountpoint == *prefix || mountpoint.starts_with(&format!("{prefix}/")))
}

/// Total and used bytes for a mounted path, from `statvfs(3)`.
fn usage(path: &str) -> Option<(u64, u64)> {
    let path = CString::new(path).ok()?;
    let mut stats: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `path` is a valid NUL-terminated string and `stats` points to
    // writable storage of the right type.
    if unsafe { libc::statvfs(path.as_ptr(), &mut stats) } != 0 {
        return None;
    }
    let fragment = stats.f_frsize as u64;
    let total = (stats.f_blocks as u64).saturating_mul(fragment);
    let free = (stats.f_bfree as u64).saturating_mul(fragment);
    Some((total.saturating_sub(free), total))
}

fn devices() -> Vec<Value> {
    let mounted = mount_entries();
    let mut out = BTreeMap::new();
    let Ok(disks) = std::fs::read_dir("/sys/block") else {
        return Vec::new();
    };
    for disk in disks.flatten() {
        let disk_name = disk.file_name().to_string_lossy().into_owned();
        if virtual_disk(&disk_name) {
            continue;
        }
        let base = format!("/sys/block/{disk_name}");
        let model = model(&base);
        let removable = read_trim(&format!("{base}/removable")) == "1";
        let mut partitions = Vec::new();
        if let Ok(children) = std::fs::read_dir(&base) {
            for child in children.flatten() {
                if child.path().join("partition").exists() {
                    partitions.push(child.file_name().to_string_lossy().into_owned());
                }
            }
        }
        partitions.sort();
        if partitions.is_empty() {
            let size = size_bytes(&base);
            out.insert(
                disk_name.clone(),
                device_json(&disk_name, &disk_name, size, &model, removable, &mounted),
            );
        } else {
            for partition in partitions {
                let size = size_bytes(&format!("{base}/{partition}"));
                out.insert(
                    partition.clone(),
                    device_json(&partition, &disk_name, size, &model, removable, &mounted),
                );
            }
        }
    }
    out.into_values().collect()
}

fn device_json(
    name: &str,
    parent: &str,
    size_bytes: u64,
    model: &str,
    removable: bool,
    mounted: &[MountEntry],
) -> Value {
    let path = format!("/dev/{name}");
    let is_mounted = mounted.iter().any(|mount| {
        mount.device == path
            || (name == parent && mount.device.starts_with(&format!("/dev/{parent}")))
    });
    json!({
        "name": name,
        "path": path,
        "size-bytes": size_bytes,
        "model": model,
        "removable": removable,
        "mounted": is_mounted,
    })
}

fn size_bytes(base: &str) -> u64 {
    read_trim(&format!("{base}/size"))
        .parse::<u64>()
        .unwrap_or(0)
        .saturating_mul(512)
}

fn model(base: &str) -> String {
    let value = read_trim(&format!("{base}/device/model"));
    if value.is_empty() {
        "Unknown".to_string()
    } else {
        value
    }
}

fn virtual_disk(name: &str) -> bool {
    ["loop", "ram", "zram", "dm-"]
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mounts_parse_and_skip_pseudo_filesystems() {
        let text = "\
/dev/sda1 / ext4 rw 0 0
proc /proc proc rw 0 0
overlay / overlay rw 0 0
/dev/sdb1 /media/usb\\040drive exfat rw 0 0
tmpfs /run tmpfs rw 0 0
";
        // `mount_entries` reads the host, so exercise the parser directly.
        let mut entries = Vec::new();
        for line in text.lines() {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let device = unescape(fields[0]);
            let mountpoint = unescape(fields[1]);
            let filesystem = fields[2].to_string();
            if is_storage_mount(&device, &mountpoint, &filesystem) {
                entries.push((device, mountpoint, filesystem));
            }
        }
        assert_eq!(entries.len(), 3);
        assert!(entries.iter().any(
            |(device, mountpoint, _)| device == "/dev/sdb1" && mountpoint == "/media/usb drive"
        ));
    }

    #[test]
    fn the_host_devices_have_the_contract_shape() {
        let list = devices();
        for device in &list {
            assert!(device["name"].is_string());
            assert!(device["path"].as_str().unwrap().starts_with("/dev/"));
            assert!(device["size-bytes"].is_number());
            assert!(device["mounted"].is_boolean());
        }
    }

    #[test]
    fn attach_and_detach_are_refused_until_the_apply_flow_exists() {
        let service = Service;
        for function in ["attach", "detach"] {
            let error = service
                .call(STORAGE, function, json!({}))
                .expect_err("read-only");
            assert_eq!(error.kind(), anth_contract::ErrorKind::Denied);
        }
    }
}
