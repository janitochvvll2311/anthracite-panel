# Storage

The daemon reads the host's block devices, mounts and swap areas. This first
Storage slice is **read-only**; mount/unmount and partition/format operations
arrive in later cuts.

Two sources answer two different questions:

- `lsblk` knows the block-device tree, filesystems, labels and UUIDs, but cannot
  see an overlay or a tmpfs.
- `/proc/self/mountinfo` knows every mount but nothing about a device that is
  present and unmounted.

`/proc/swaps` is the only source for active swap areas. A snapshot carries all
three and links devices to mounts by source path.

## Configuration

```toml
[volumes]
control = false
partition = false
format = false
mount_root = "/mnt"
disk_allow = []
```

The inventory always works. The flags are reported in every snapshot so the UI
can show what the daemon will allow once mutations exist:

- `control` will gate mount/unmount.
- `partition` will gate partition-table edits.
- `format` will gate `mkfs`.
- `mount_root` is the directory new mounts must stay under.
- `disk_allow` is the list of whole disks partition/format may touch.

## Requirements

- `lsblk` from util-linux. If it is missing, the snapshot degrades to mounts
  only and carries a warning; reads still work.
- `/proc/self/mountinfo` and `/proc/swaps`. An unreadable source is a warning,
  not a failed snapshot.
- No capability is needed for the read-only inventory. Mutations will need
  `CAP_SYS_ADMIN`/root and validate against a fresh snapshot.

## Data model

### Device

| Field | Meaning |
| --- | --- |
| `name`, `path` | Kernel name and device path (`sda1`, `/dev/sda1`) |
| `kind` | `disk`, `part`, `crypt`, `lvm`, `raid`, `loop`, `rom` or `other` |
| `parent` | Parent device name for the tree; absent for a disk |
| `size_bytes` | Size in bytes |
| `fstype`, `label`, `uuid` | Filesystem metadata when present |
| `mountpoints` | Every mountpoint using the device; empty when unmounted |
| `read_only`, `removable` | Device flags |
| `model`, `transport` | Hardware metadata when `lsblk` reports it |

### Mount

| Field | Meaning |
| --- | --- |
| `mountpoint` | Path from mountinfo, octal escapes decoded |
| `source` | Device path, filesystem name or network share |
| `fstype` | Kernel filesystem name |
| `options` | Comma-separated mount options |
| `read_only` | `ro` in the mount or superblock options |
| `virtual_fs` | Kernel/pseudo filesystem (`proc`, `tmpfs`, `overlay`, ...) |
| `usage` | Best-effort `statvfs` totals; absent when unavailable |

### Snapshot

```json
{
  "devices": [
    {
      "name": "sda1",
      "path": "/dev/sda1",
      "kind": "part",
      "parent": "sda",
      "size_bytes": 80396402176,
      "fstype": "ext4",
      "label": "root",
      "uuid": "...",
      "mountpoints": ["/"],
      "read_only": false,
      "removable": false
    }
  ],
  "mounts": [
    {
      "mountpoint": "/",
      "source": "/dev/sda1",
      "fstype": "ext4",
      "options": "rw,relatime",
      "read_only": false,
      "virtual_fs": false,
      "usage": {
        "total_bytes": 80396402176,
        "used_bytes": 37537841152,
        "available_bytes": 38247329792
      }
    }
  ],
  "swap_devices": ["/dev/sda2"],
  "control_enabled": false,
  "partition_enabled": false,
  "format_enabled": false,
  "mount_root": "/mnt",
  "disk_allow": [],
  "timestamp_ms": 1790000000000
}
```

A degraded read adds `warning`; it never silently drops a source. Devices and
mounts are sent in full; there is no truncation pass.

## HTTP

`GET /api/volumes` requires a live panel session unless auth is disabled and
returns the snapshot. It is read-only and not rate-limited.

## Socket

```json
{ "type": "volumes" }
```

The answer is `{"type":"volumes","data":{...}}`. Socket and HTTP share the same
`Daemon::execute` dispatch.

## CLI

```bash
anth-panel storage list
anth-panel storage ls
anth-panel volumes list
```

The command prints the policy flags, a device tree, the mounts with
used/total space, and any active swap. `volumes` is an alias for `storage`.

## Web UI

**Storage** is one sidebar section with **Volumes** and **Partitions** tabs. It
reads on connect and on Refresh; there is no SSE in this read-only slice.

- Toolbar: loading/ready/error state, device or partition count, Refresh.
- Context strip: mount root and the `control`/`partition`/`format` policy flags.
- Filter: matches device name, path, filesystem, label, model and mountpoints;
  in the mounts list it also matches source, filesystem and options.
- Volumes tab:
  - One card per device, indented by parent depth, with kind, size, filesystem,
    label/model/transport, flags (`read-only`, `removable`, `swap`) and
    mountpoint chips.
  - Mounts section: one row per mount with source, filesystem, virtual-fs flag,
    read-only flag and a used/total bar when `statvfs` succeeds.
  - Swap section when active swap areas exist.
- Partitions tab: a flat list of partitions with their parent disk, size,
  filesystem and mountpoints.
- A degraded source shows a warning above the inventory; an empty inventory has
  its own sentence rather than an error.
- Narrow screens collapse cards to one column and keep the Refresh control at
  44 px.
