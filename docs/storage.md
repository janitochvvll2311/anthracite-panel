# Storage

The daemon reads the host's block devices, mounts and swap areas. This slice
adds **mount and unmount**; partition-table edits and `mkfs` arrive in later
cuts.

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
can show what the daemon allows:

- `control` gates mount and unmount.
- `partition` gates partition-table edits (next cut).
- `format` gates `mkfs` (next cut).
- `mount_root` is the directory new mounts must stay under.
- `disk_allow` is the list of whole disks partition/format may touch.

## Requirements

- `lsblk` from util-linux. If it is missing, the snapshot degrades to mounts
  only and carries a warning; reads still work.
- `/proc/self/mountinfo` and `/proc/swaps`. An unreadable source is a warning,
  not a failed snapshot.
- `mount` and `umount` on `PATH`. A missing binary is `501 mount_unavailable`.
- `CAP_SYS_ADMIN` (or root) in the daemon's process for mount/unmount. The
  inventory itself needs no capability.

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

## Mutations

Both mutations require `[volumes] control`. They read a fresh snapshot before
checking a guard.

### Mount

```json
{
  "device": "/dev/sdb1",
  "target": "/mnt/data",
  "fs_type": "ext4",
  "read_only": false,
  "allow_removable": false
}
```

The daemon validates:

- the device is a known block device on this host and is not already mounted;
- a removable device requires `allow_removable`;
- the target is absolute, has no `..`, exists, is a directory and is not
  already a mountpoint;
- the canonical target is under the canonical `[volumes] mount_root`;
- the filesystem type is explicit or detected by `lsblk`, at most 32
  alphanumerics/`_`/`-`/`.`.

It then runs `mount [-o ro] -t TYPE DEVICE TARGET` directly, with each value in
its own argv element.

### Unmount

```json
{ "target": "/mnt/data" }
```

The target must be an absolute current mountpoint in a fresh mountinfo. The
daemon refuses `/`, virtual filesystems, and any path that contains a protected
path: the file root, socket, config, state/secrets/sessions/plugins, or the
running executable. It then runs `umount TARGET`.

Kernel refusals are classified: a busy mount is `409 mount_busy`, a permission
failure `403 mount_permission`, a missing mount `404 mount_not_found`, an
unsupported filesystem/option `400 mount_refused`, and a spawn failure `501
mount_unavailable`.

## HTTP

`GET /api/volumes` requires a live panel session unless auth is disabled and
returns the snapshot. It is read-only and not rate-limited.

### `POST /api/volumes/mount`

The body is the mount request above. `200 {"message":"mounted /dev/sdb1 on /mnt/data as ext4"}`.
Requires `[volumes] control`.

### `POST /api/volumes/unmount`

```json
{ "target": "/mnt/data" }
```

`200 {"message":"unmounted /mnt/data"}`. Requires `[volumes] control`.

## Socket

```json
{ "type": "volumes" }
```

The answer is `{"type":"volumes","data":{...}}`. Mutations use the same
dispatch:

```json
{
  "type": "volumes_mount",
  "data": {
    "device": "/dev/sdb1",
    "target": "/mnt/data",
    "fs_type": "ext4",
    "read_only": false,
    "allow_removable": false
  }
}
{ "type": "volumes_unmount", "data": { "target": "/mnt/data" } }
```

Both answer `{"type":"volume_changed","data":{"message":"..."}}`. Socket and
HTTP share the same `Daemon::execute` dispatch.

## CLI

```bash
anth-panel storage list
anth-panel storage ls
anth-panel volumes list
anth-panel storage mount /dev/sdb1 /mnt/data --type ext4 --yes
anth-panel storage mount /dev/sdb1 /mnt/data --read-only --yes
anth-panel storage unmount /mnt/data --yes
```

`list` prints the policy flags, a device tree, the mounts with used/total space,
and any active swap. `volumes` is an alias for `storage`. `mount` and `unmount`
require `[volumes] control`, ask for confirmation on a TTY and refuse without
`--yes` when stdin is not a terminal. `--allow-removable` is required for
removable media.

## Web UI

**Storage** is one sidebar section with **Volumes** and **Partitions** tabs. It
reads on connect, after every mutation, and on Refresh; there is no SSE.

- Toolbar: loading/ready/error state, device or partition count, Refresh.
- Context strip: mount root and the `control`/`partition`/`format` policy flags.
- Filter: matches device name, path, filesystem, label, model and mountpoints;
  in the mounts list it also matches source, filesystem and options.
- Volumes tab:
  - One card per device, indented by parent depth, with kind, size, filesystem,
    label/model/transport, flags (`read-only`, `removable`, `swap`) and
    mountpoint chips.
  - A mount form when control is on: device picker (unmounted devices only),
    target under the mount root, optional type, read-only and allow-removable.
  - Mounts section: one row per mount with source, filesystem, virtual-fs flag,
    read-only flag and a used/total bar when `statvfs` succeeds. A non-virtual
    mount gets an Unmount button with an inline confirmation.
  - Swap section when active swap areas exist.
- Partitions tab: a flat list of partitions with their parent disk, size,
  filesystem and mountpoints. Partition/format controls arrive later.
- A degraded source shows a warning above the inventory; an empty inventory has
  its own sentence rather than an error. A mutation error keeps the last good
  snapshot and shows the daemon message above it.
- With control off, the mount form and Unmount buttons are absent.
- Narrow screens collapse cards to one column and keep controls at 44 px.
