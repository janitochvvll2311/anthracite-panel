//! Host storage service for the Storage app.
//!
//! The read half lists block devices from `/sys/block` and the mounts from
//! `/proc/self/mounts`, with usage from `statvfs(3)`. `attach` and `detach`
//! validate the inputs and run `mount`/`umount`; they need `CAP_SYS_ADMIN`.

use std::collections::BTreeMap;
use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::process::Command;

use anth_contract::Error;
use anth_service::Handler;
use serde_json::{json, Value};

const STORAGE: &str = "anthracite:storage/storage@1.2.0";

struct Service {
    mount_root: PathBuf,
}

impl Handler for Service {
    fn call(&self, interface: &str, function: &str, input: Value) -> Result<Value, Error> {
        if interface != STORAGE {
            return Err(Error::not_found(format!(
                "`{interface}` is not exported by the storage service"
            )));
        }
        match function {
            "devices" => Ok(Value::Array(devices())),
            "mounts" => Ok(Value::Array(mounts())),
            "attach" => self.attach(&input),
            "detach" => self.detach(&input),
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
    let root = match mount_root_argument() {
        Ok(root) => root,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    let service = Service::new(root);
    if let Err(error) = anth_service::serve(&socket, std::sync::Arc::new(service)).await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn mount_root_argument() -> Result<PathBuf, Error> {
    let mut args = std::env::args().skip(1);
    let mut root = PathBuf::from("/media");
    while let Some(argument) = args.next() {
        if argument == "--mount-root" {
            root = args
                .next()
                .map(PathBuf::from)
                .ok_or_else(|| Error::invalid("`--mount-root` needs a path"))?;
        } else if argument == "--socket" {
            args.next();
        }
    }
    std::fs::create_dir_all(&root).map_err(|error| {
        Error::unavailable(format!("creating mount root {}: {error}", root.display()))
    })?;
    std::fs::canonicalize(&root).map_err(|error| {
        Error::unavailable(format!("resolving mount root {}: {error}", root.display()))
    })
}

impl Service {
    fn new(mount_root: PathBuf) -> Self {
        Self { mount_root }
    }

    fn attach(&self, input: &Value) -> Result<Value, Error> {
        let device = text(input, "device")?;
        let mountpoint = self.validate_mountpoint(&text(input, "mountpoint")?, true)?;
        let filesystem = text(input, "filesystem")?;
        if filesystem.is_empty()
            || !filesystem.chars().all(|character| {
                character.is_ascii_alphanumeric() || character == '_' || character == '-'
            })
        {
            return Err(Error::invalid("`filesystem` is not a filesystem name"));
        }
        if !known_device(&device) {
            return Err(Error::not_found(format!(
                "`{device}` is not a block device"
            )));
        }
        if device_mounted(&device) {
            return Err(Error::conflict(format!("`{device}` is already mounted")));
        }
        if let Some(other) = overlapping_mount(&mountpoint, &mount_entries()) {
            return Err(Error::conflict(format!(
                "`{mountpoint}` overlaps the mountpoint `{}`",
                other.mountpoint
            )));
        }
        run("mount", &mount_args(&filesystem, &device, &mountpoint))?;
        Ok(Value::Null)
    }

    fn detach(&self, input: &Value) -> Result<Value, Error> {
        let mountpoint = self.validate_mountpoint(&text(input, "mountpoint")?, false)?;
        if system_mount(&mountpoint) {
            return Err(Error::denied(format!(
                "`{mountpoint}` is a system mount and cannot be unmounted"
            )));
        }
        let mounts = mount_entries();
        if !mounts.iter().any(|mount| mount.mountpoint == mountpoint) {
            return Err(Error::not_found(format!(
                "`{mountpoint}` is not a mountpoint"
            )));
        }
        if let Some(inner) = inner_mount(&mountpoint, &mounts) {
            return Err(Error::conflict(format!(
                "`{mountpoint}` contains the mountpoint `{}`; unmount it first",
                inner.mountpoint
            )));
        }
        run("umount", &umount_args(&mountpoint))?;
        Ok(Value::Null)
    }

    /// The mountpoint must resolve inside the operator-approved mount root.
    fn validate_mountpoint(&self, mountpoint: &str, must_exist: bool) -> Result<String, Error> {
        if !mountpoint.starts_with('/') || mountpoint.contains('\0') {
            return Err(Error::invalid("`mountpoint` must be an absolute path"));
        }
        let canonical = std::fs::canonicalize(mountpoint)
            .map_err(|_| Error::invalid("`mountpoint` must be an existing directory"))?;
        if !canonical.starts_with(&self.mount_root) {
            return Err(Error::denied(format!(
                "`{}` is outside the mount root {}",
                canonical.display(),
                self.mount_root.display()
            )));
        }
        if canonical == Path::new("/") {
            return Err(Error::denied("the root filesystem cannot be changed"));
        }
        if must_exist && !canonical.is_dir() {
            return Err(Error::invalid("`mountpoint` must be a directory"));
        }
        Ok(canonical.to_string_lossy().into_owned())
    }
}

struct MountEntry {
    device: String,
    mountpoint: String,
    filesystem: String,
    used_bytes: u64,
    total_bytes: u64,
}

fn system_mount(mountpoint: &str) -> bool {
    ["/", "/boot", "/boot/efi", "/proc", "/sys", "/dev", "/run"]
        .iter()
        .any(|prefix| mountpoint == *prefix || mountpoint.starts_with(&format!("{prefix}/")))
}

/// The mount that shares a path with `mountpoint`, either above or below it.
/// The root mount is the base tree, not an overlay, so it never conflicts.
fn overlapping_mount<'a>(mountpoint: &str, mounts: &'a [MountEntry]) -> Option<&'a MountEntry> {
    let root = mountpoint.trim_end_matches('/');
    mounts.iter().find(|mount| {
        let existing = mount.mountpoint.trim_end_matches('/');
        !existing.is_empty()
            && (existing == root
                || existing.starts_with(&format!("{root}/"))
                || root.starts_with(&format!("{existing}/")))
    })
}

/// A mount below `mountpoint`; unmounting the parent first would hide it.
fn inner_mount<'a>(mountpoint: &str, mounts: &'a [MountEntry]) -> Option<&'a MountEntry> {
    let root = mountpoint.trim_end_matches('/');
    mounts.iter().find(|mount| {
        let existing = mount.mountpoint.trim_end_matches('/');
        !existing.is_empty() && existing != root && existing.starts_with(&format!("{root}/"))
    })
}

fn known_device(device: &str) -> bool {
    devices()
        .iter()
        .any(|candidate| candidate["path"].as_str() == Some(device))
}

fn device_mounted(device: &str) -> bool {
    devices()
        .iter()
        .any(|candidate| candidate["path"].as_str() == Some(device) && candidate["mounted"] == true)
}

fn mount_args(filesystem: &str, device: &str, mountpoint: &str) -> Vec<String> {
    vec![
        "-t".to_string(),
        filesystem.to_string(),
        device.to_string(),
        mountpoint.to_string(),
    ]
}

fn umount_args(mountpoint: &str) -> Vec<String> {
    vec![mountpoint.to_string()]
}

fn run(program: &str, args: &[String]) -> Result<(), Error> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|error| Error::unavailable(format!("running `{program}`: {error}")))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let message = if stderr.is_empty() {
        format!("`{program}` exited with {}", output.status)
    } else {
        stderr
    };
    if message.contains("Operation not permitted")
        || message.to_ascii_lowercase().contains("permission denied")
    {
        return Err(Error::denied(format!(
            "the storage apply flow needs CAP_SYS_ADMIN: {message}"
        )));
    }
    Err(Error::unavailable(format!(
        "`{program} {}` failed: {message}",
        args.join(" ")
    )))
}

fn text(input: &Value, key: &str) -> Result<String, Error> {
    input
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| Error::invalid(format!("`{key}` is required")))
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
    let mut out = Vec::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' && index + 4 <= bytes.len() {
            let digits = &bytes[index + 1..index + 4];
            if digits.iter().all(|byte| (b'0'..=b'7').contains(byte)) {
                let octal = (digits[0] - b'0') * 64 + (digits[1] - b'0') * 8 + (digits[2] - b'0');
                out.push(octal);
                index += 4;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
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
    fn mountpoint_escapes_keep_utf8_and_spaces() {
        assert_eq!(unescape("/media/datos/ñandú\\040x"), "/media/datos/ñandú x");
        assert_eq!(unescape("/media/plain"), "/media/plain");
    }

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
    fn mount_arguments_and_validation() {
        assert_eq!(
            mount_args("ext4", "/dev/sda1", "/mnt/x"),
            vec!["-t", "ext4", "/dev/sda1", "/mnt/x"]
        );
        assert_eq!(umount_args("/mnt/x"), vec!["/mnt/x"]);
        let root = std::env::temp_dir()
            .join("anthracite-panel")
            .join(format!("storage-root-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("mnt")).expect("mount root");
        let canonical = std::fs::canonicalize(&root).expect("canonical root");
        let inside = canonical.join("mnt").to_string_lossy().into_owned();
        let service = Service::new(canonical);
        let error = service
            .call(
                STORAGE,
                "attach",
                json!({"device": "/dev/nope", "mountpoint": inside, "filesystem": "ext4"}),
            )
            .expect_err("unknown device");
        assert_eq!(error.kind(), anth_contract::ErrorKind::NotFound);
        let error = service
            .call(
                STORAGE,
                "attach",
                json!({"device": "/dev/nope", "mountpoint": "relative", "filesystem": "ext4"}),
            )
            .expect_err("relative mountpoint");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Invalid);
        let error = service
            .call(
                STORAGE,
                "attach",
                json!({"device": "/dev/nope", "mountpoint": "/tmp", "filesystem": "ext4"}),
            )
            .expect_err("outside the mount root");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Denied);
        let error = service
            .call(STORAGE, "detach", json!({"mountpoint": "/"}))
            .expect_err("root");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Denied);
        assert!(system_mount("/boot/efi"));
        assert!(!system_mount("/media/sd"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn overlapping_mounts_are_rejected() {
        let mounts = vec![
            MountEntry {
                device: "/dev/sda1".to_string(),
                mountpoint: "/".to_string(),
                filesystem: "ext4".to_string(),
                used_bytes: 0,
                total_bytes: 1,
            },
            MountEntry {
                device: "/dev/sdb1".to_string(),
                mountpoint: "/media/demo".to_string(),
                filesystem: "exfat".to_string(),
                used_bytes: 0,
                total_bytes: 1,
            },
        ];
        assert_eq!(
            overlapping_mount("/media", &mounts).map(|mount| mount.mountpoint.as_str()),
            Some("/media/demo")
        );
        assert_eq!(
            overlapping_mount("/media/demo/sub", &mounts).map(|mount| mount.mountpoint.as_str()),
            Some("/media/demo")
        );
        assert_eq!(
            overlapping_mount("/media/demo", &mounts).map(|mount| mount.mountpoint.as_str()),
            Some("/media/demo")
        );
        assert!(overlapping_mount("/home", &mounts).is_none());
        assert_eq!(
            inner_mount("/media", &mounts).map(|mount| mount.mountpoint.as_str()),
            Some("/media/demo")
        );
        assert!(inner_mount("/media/demo", &mounts).is_none());
    }
}
