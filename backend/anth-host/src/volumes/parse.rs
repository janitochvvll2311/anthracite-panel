//! Pure parsers for `lsblk` JSON, `/proc/self/mountinfo` and `/proc/swaps`.
//!
//! Keeping these functions over strings makes the fixtures cover the awkward
//! cases: the modern and legacy `MOUNTPOINTS` columns, numeric strings from
//! older util-linux, escaped mountinfo paths and device trees.

use std::collections::HashSet;

use anth_wire::{VolumeDevice, VolumeKind, VolumeMount};
use serde::Deserialize;
use serde_json::Value;

/// Filesystems with no block device behind them. They are reported but never
/// valid mount or unmount targets.
pub const VIRTUAL_FILESYSTEMS: &[&str] = &[
    "autofs",
    "bpf",
    "cgroup",
    "cgroup2",
    "configfs",
    "debugfs",
    "devpts",
    "devtmpfs",
    "efivarfs",
    "fusectl",
    "hugetlbfs",
    "mqueue",
    "nsfs",
    "overlay",
    "proc",
    "pstore",
    "ramfs",
    "rpc_pipefs",
    "securityfs",
    "squashfs",
    "sysfs",
    "tmpfs",
    "tracefs",
];

/// `MOUNTPOINTS` replaced `MOUNTPOINT` in util-linux 2.37. Modern first.
pub const LSBLK_COLUMNS: &str =
    "NAME,PATH,TYPE,SIZE,FSTYPE,LABEL,UUID,MOUNTPOINTS,RO,RM,MODEL,TRAN";
pub const LSBLK_COLUMNS_LEGACY: &str =
    "NAME,PATH,TYPE,SIZE,FSTYPE,LABEL,UUID,MOUNTPOINT,RO,RM,MODEL,TRAN";

#[derive(Debug, Default, Deserialize)]
struct RawDevice {
    #[serde(default)]
    name: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    size: Option<Value>,
    #[serde(default)]
    fstype: Option<String>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    uuid: Option<String>,
    #[serde(default)]
    mountpoint: Option<String>,
    #[serde(default)]
    mountpoints: Option<Vec<Option<String>>>,
    #[serde(default)]
    ro: Option<Value>,
    #[serde(default)]
    rm: Option<Value>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    tran: Option<String>,
    #[serde(default)]
    children: Vec<RawDevice>,
}

/// Parses one `lsblk -J` document into a flat list with parent links.
pub fn parse_lsblk(json: &str) -> Result<Vec<VolumeDevice>, String> {
    let document: Value =
        serde_json::from_str(json).map_err(|error| format!("invalid lsblk JSON: {error}"))?;
    let devices = document
        .get("blockdevices")
        .ok_or_else(|| "lsblk JSON has no `blockdevices` array".to_string())?
        .clone();
    let devices: Vec<RawDevice> = serde_json::from_value(devices)
        .map_err(|error| format!("unexpected lsblk shape: {error}"))?;

    let mut flattened = Vec::new();
    for device in &devices {
        flatten(device, None, &mut flattened);
    }
    Ok(flattened)
}

fn flatten(device: &RawDevice, parent: Option<&str>, out: &mut Vec<VolumeDevice>) {
    let name = device.name.trim();
    if name.is_empty() {
        return;
    }
    out.push(VolumeDevice {
        name: name.to_string(),
        path: device
            .path
            .clone()
            .filter(|path| !path.is_empty())
            .unwrap_or_else(|| format!("/dev/{name}")),
        kind: kind_from_str(device.kind.as_deref().unwrap_or_default()),
        parent: parent.map(str::to_string),
        size_bytes: as_u64(&device.size),
        fstype: clean(device.fstype.clone()),
        label: clean(device.label.clone()),
        uuid: clean(device.uuid.clone()),
        mountpoints: mountpoints(device),
        read_only: as_bool(&device.ro),
        removable: as_bool(&device.rm),
        model: clean(device.model.clone()),
        transport: clean(device.tran.clone()),
    });
    for child in &device.children {
        flatten(child, Some(name), out);
    }
}

fn kind_from_str(value: &str) -> VolumeKind {
    value.parse().unwrap_or(VolumeKind::Other)
}

fn mountpoints(device: &RawDevice) -> Vec<String> {
    let points: Vec<String> = match &device.mountpoints {
        Some(list) => list.iter().flatten().cloned().collect(),
        None => device.mountpoint.clone().into_iter().collect(),
    };
    let mut seen = HashSet::new();
    points
        .into_iter()
        .filter(|point| !point.is_empty() && seen.insert(point.clone()))
        .collect()
}

fn clean(value: Option<String>) -> Option<String> {
    value
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

fn as_u64(value: &Option<Value>) -> u64 {
    match value {
        Some(Value::Number(number)) => number.as_u64().unwrap_or(0),
        Some(Value::String(text)) => text.trim().parse().unwrap_or(0),
        _ => 0,
    }
}

fn as_bool(value: &Option<Value>) -> bool {
    match value {
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => number.as_u64().unwrap_or(0) != 0,
        Some(Value::String(text)) => matches!(text.trim(), "1" | "true"),
        _ => false,
    }
}

/// Parses `/proc/swaps` into the device paths that are live swap areas.
pub fn swaps_from(text: &str) -> Vec<String> {
    text.lines()
        .skip(1)
        .filter_map(|line| line.split_whitespace().next())
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .collect()
}

/// Parses `/proc/self/mountinfo`. Usage is filled in by the service.
pub fn parse_mountinfo(text: &str) -> Vec<VolumeMount> {
    text.lines().filter_map(parse_mountinfo_line).collect()
}

fn parse_mountinfo_line(line: &str) -> Option<VolumeMount> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.len() < 6 {
        return None;
    }
    let separator = fields.iter().position(|field| *field == "-")?;
    let mountpoint = unescape(fields[4]);
    let options = fields[5].to_string();
    let fstype = fields.get(separator + 1)?.to_string();
    let source = fields.get(separator + 2).map(|value| unescape(value))?;
    let super_options = fields.get(separator + 3).copied().unwrap_or_default();
    let read_only = has_option(&options, "ro") || has_option(super_options, "ro");

    Some(VolumeMount {
        mountpoint,
        source,
        virtual_fs: VIRTUAL_FILESYSTEMS.contains(&fstype.as_str()),
        fstype,
        options,
        read_only,
        usage: None,
    })
}

fn has_option(options: &str, wanted: &str) -> bool {
    options.split(',').any(|option| option == wanted)
}

/// `/proc/self/mountinfo` escapes space, tab, newline and backslash as octal.
fn unescape(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' && index + 3 < bytes.len() {
            let digits = &bytes[index + 1..index + 4];
            if digits.iter().all(|digit| (b'0'..=b'7').contains(digit)) {
                let code = digits
                    .iter()
                    .fold(0u32, |acc, digit| acc * 8 + u32::from(digit - b'0'));
                if let Ok(code) = u8::try_from(code) {
                    out.push(code);
                    index += 4;
                    continue;
                }
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LSBLK: &str = r#"{
      "blockdevices": [
        {
          "name": "sda", "path": "/dev/sda", "type": "disk", "size": 1000,
          "fstype": null, "label": null, "uuid": null,
          "mountpoints": [null], "ro": false, "rm": false,
          "model": "Example Disk  ", "tran": "sata",
          "children": [
            {
              "name": "sda1", "path": "/dev/sda1", "type": "part", "size": "500",
              "fstype": "ext4", "label": "root", "uuid": "abc",
              "mountpoints": ["/", "/", "/boot"], "ro": "1", "rm": "0",
              "model": null, "tran": null
            },
            {
              "name": "sda2", "path": null, "type": "crypt", "size": 200,
              "fstype": "crypto_LUKS", "label": null, "uuid": null,
              "mountpoint": "/mnt/data", "ro": 0, "rm": 0
            }
          ]
        },
        {
          "name": "loop0", "type": "loop", "size": 10,
          "mountpoints": [], "ro": true
        }
      ]
    }"#;

    #[test]
    fn lsblk_flattens_the_tree_and_cleans_fields() {
        let devices = parse_lsblk(LSBLK).expect("parse");
        assert_eq!(devices.len(), 4);
        assert_eq!(devices[0].name, "sda");
        assert_eq!(devices[0].kind, VolumeKind::Disk);
        assert_eq!(devices[0].model.as_deref(), Some("Example Disk"));
        assert_eq!(devices[0].transport.as_deref(), Some("sata"));
        assert_eq!(devices[1].parent.as_deref(), Some("sda"));
        assert_eq!(devices[1].size_bytes, 500);
        assert_eq!(devices[1].mountpoints, vec!["/", "/boot"]);
        assert!(devices[1].read_only);
        assert_eq!(devices[2].path, "/dev/sda2");
        assert_eq!(devices[2].kind, VolumeKind::Crypt);
        assert_eq!(devices[2].mountpoints, vec!["/mnt/data"]);
        assert_eq!(devices[3].path, "/dev/loop0");
        assert!(devices[3].read_only);
    }

    #[test]
    fn lsblk_rejects_a_document_without_devices() {
        assert!(parse_lsblk("{}").is_err());
        assert!(parse_lsblk("not json").is_err());
    }

    #[test]
    fn lsblk_accepts_a_legacy_mountpoint_column() {
        let json = r#"{"blockdevices":[{"name":"sdb","type":"disk","size":1,"mountpoint":"/x"}]}"#;
        let devices = parse_lsblk(json).expect("parse");
        assert_eq!(devices[0].mountpoints, vec!["/x"]);
    }

    #[test]
    fn mountinfo_unescapes_paths_and_marks_virtual_filesystems() {
        let text = "\
36 25 0:30 / / rw,relatime shared:1 - overlay overlay rw,lowerdir=/lower
37 36 0:31 / /proc rw,nosuid - proc proc rw
38 36 8:1 / /media/My\\040Disk ro,relatime - ext4 /dev/sda1 ro
";
        let mounts = parse_mountinfo(text);
        assert_eq!(mounts.len(), 3);
        assert_eq!(mounts[0].mountpoint, "/");
        assert!(mounts[0].virtual_fs);
        assert!(mounts[1].virtual_fs);
        assert_eq!(mounts[2].mountpoint, "/media/My Disk");
        assert_eq!(mounts[2].source, "/dev/sda1");
        assert!(!mounts[2].virtual_fs);
        assert!(mounts[2].read_only);
    }

    #[test]
    fn swaps_skip_the_header_and_keep_file_backed_areas() {
        let text = "Filename\tType\tSize\tUsed\tPriority\n/dev/sda2 partition 1000 0 -2\n/swapfile file 2000 0 -3\n";
        assert_eq!(swaps_from(text), vec!["/dev/sda2", "/swapfile"]);
    }
}
