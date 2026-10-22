# Volumes

The daemon can report the storage inventory of the host and, when explicitly
allowed, mount and unmount filesystems. Showing the inventory is read-only.

The panel has no authentication yet, so keep the HTTP port on a trusted network
or behind a proxy that authenticates users. Anyone who can reach the API can
mount or unmount filesystems when control is enabled.

## What the inventory contains

Two data sources feed one snapshot:

- `lsblk --json --bytes` lists block devices: disks, partitions, LUKS
  containers, LVM logical volumes, RAID members, loop devices and optical
  drives. It also reports the detected filesystem, label, UUID, whether the
  device is read-only or removable, and the model and transport when the kernel
  exposes them.
- `/proc/self/mountinfo` lists **every mount** in the daemon's mount namespace,
  including the virtual filesystems (`proc`, `sysfs`, `cgroup2`, `tmpfs`,
  `devpts`, `mqueue`, overlays) and network filesystems. Mounts are not filtered
  by type: an operator looking for a specific mount should be able to find it.

The two lists are reported separately. Joining them would hide mounts whose
source is not a block device (`overlay`, `server:/export`) or devices that are
not mounted at all, which is precisely what the mount form needs.

`lsblk` output is flattened, and each device keeps its `parent` name, so a
client can rebuild the tree without a second query.

Both lists are bounded by the protocol line cap. When a snapshot does not fit,
the daemon first drops the longest diagnostic fields, then shortens the device
list and finally the mount list. `total` keeps the real count and `warning`
explains what was dropped, so a shortened snapshot never reads as a complete
one. Clients label the count as `shown of total` when `truncated` is true. The
budget is measured on the framed response, `{"type":…,"data":…}` included, which
is the string that actually has to fit a line.

Truncation is a display concern only. Mount and unmount validate against the
full inventory, so a device or mountpoint past the cutoff is still reachable;
the guards never depend on how much of the snapshot survived.

The snapshot also reports `control_enabled`, which tells a client whether the
control buttons should be offered. It is derived from the daemon's config, not
from the caller.

## Configuration

Volume control is **off by default**:

```toml
sudo = false

[volumes]
control = false
mount_root = "/mnt"
```

Set `control = true` to allow mount and unmount requests. The top-level
`sudo = true` runs `mount` and `umount` as `sudo -n`, which is meant for
development when the daemon runs as an unprivileged user with a `NOPASSWD`
sudoers rule for those binaries:

```toml
sudo = true

[volumes]
control = true
```

Without sudo, the daemon calls `mount` and `umount` directly and relies on
`CAP_SYS_ADMIN` or root, or on an `/etc/fstab` entry marked `user` or `users`.

`mount_root` is the ceiling for mount targets. A target must resolve inside it,
after following symlinks, or the request is refused. This stops a caller with
API access from mounting a device over `/etc`, `/usr` or any other path the host
depends on. Set it to an empty string to disable the check; that is the
documented escape hatch for a host that mounts under `/srv` or a data disk.

The debug default config enables `control` and keeps `mount_root = "/mnt"`.
Release builds leave control off.

## Mount policy

A mount request is validated against a **fresh** snapshot before any command
runs:

1. The device must be a block device the daemon can see. Either the full path
   (`/dev/sdb1`) or the kernel name (`sdb1`) is accepted.
2. The device must not already be mounted.
3. Removable media is refused unless the request sets `allow_removable`. An
   unplugged USB device mid-write is a failure the panel should not invite.
4. The target must be absolute, must not contain `..` and must be an existing
   directory that is not already a mountpoint.
5. When `mount_root` is set, the resolved target must be inside it.
6. The filesystem type comes from the request or from `lsblk`. A device with no
   detected filesystem needs an explicit type.
7. The filesystem type must look like one: letters, digits, dot, underscore and
   dash, at most 32 characters. `mount` is executed with an argument vector and
   never through a shell, but an argument such as `--bind` would still be read
   as an option.

The validated request becomes `mount -t <type> [-o ro] <device> <target>`.
Read-only mounts pass `-o ro`; nothing else is exposed yet.

## Unmount policy

An unmount request is validated against a fresh snapshot too:

1. The target must be absolute and, after resolving symlinks, an existing
   mountpoint.
2. `/` is always refused.
3. Virtual filesystems (`proc`, `sysfs`, `tmpfs`, `cgroup2`, …) are refused.
   They are kernel interfaces, not storage, and unmounting one breaks the
   process that owns it.
4. The mount must not be an ancestor of a path the panel needs: its Unix
   socket, its config file, the directory served by the file explorer and the
   running executable. Unmounting `/projects` would take the panel's own
   binary offline, so it is refused with the offending path in the message. The
   config path is the file the daemon loaded from `--config`, which is what the
   shipped container passes, not `ANTH_CONFIG`.

The validated request becomes `umount <target>`. There is no lazy or forced
unmount: a busy filesystem returns an error rather than losing writes.

## Partitioning

Partition edits are a separate feature with separate switches, because they are
the only operations here that rewrite a device rather than its mount state.

`[volumes] partition` enables table edits (`create`, `resize`, `delete`).
`[volumes] format` enables `mkfs`, and is a second switch on purpose: replacing a
partition table is recoverable, formatting a partition is not.

`[volumes] disk_allow` is the authority, not the switches. It lists the whole
disks a request may touch, by path (`"/dev/sdb"`). An **empty list denies every
disk** — enabling `partition` without naming a disk changes nothing. The
shipped default is empty.

Every request is validated against a fresh snapshot before any tool runs:

1. The disk must be absolute, free of `.`/`..`/empty segments, and present in
   the inventory as a **whole disk**. A partition path is refused where a disk
   is required, so `/dev/sdb1` cannot be mistaken for `/dev/sdb`.
2. The disk must appear in `disk_allow`, matched by exact path. A prefix match
   is not used: `/dev/sdaa` must never be validated as `/dev/sda`.
3. The disk must not carry a mount this feature protects: `/`, a protected
   path, or any mounted partition. The check reads the mount's **source device
   name**, not a string prefix of the disk path, and reports the offending
   mountpoint.
4. A `create` size must be a non-zero whole number of 512-byte sectors.
5. A `format` target must be a partition of an allowlisted disk and unmounted.

Sizes and offsets are numbers, never strings, so they cannot reach a shell. The
plan is an argument vector plus an optional script on **stdin**:

| Operation | Call |
| --- | --- |
| create | `sfdisk --no-reread --append <disk>` + `size=<sectors>,type=<kind>` |
| resize | `sfdisk --no-reread -N <n> <disk>` + `size=<sectors>` |
| delete | `sfdisk --no-reread --delete <disk> <n>` |
| format | `mkfs.<type> [-L \| -n] <label> <partition>` or `mkswap` |

Each of these is a separate invocation because `sfdisk`'s input format cannot
express a partition selection. Two details are load-bearing:

- **`--append` on create.** Without it `sfdisk` writes a fresh disklabel, so
  adding a fourth partition silently deletes the first three — and exits `0`.
- **`-N` on resize, `--delete` on delete.** `<n>,size=` and `delete <n>` are not
  input-format lines; `sfdisk` answers "unsupported command" and exits `1`.

`mkfs.fat` takes `-n` for a label, not `-L`, and caps it at 11 characters; the
other tools take `-L` up to 32. The planner picks the flag and limit from the
type, so a label the tool would reject never reaches it.

A filesystem label is restricted to a conservative character set because it is
passed as an argument to a tool that would otherwise accept option-like text.

The guard reads the **unfitted** snapshot (`collect_for_mutation`), like
`mount`/`umount`. A guard reading the fitted one could pass on a disk whose
mounted partition had been dropped to fit the line cap.

`mkfs.<type>` derives from an allowlist (`ext4`, `xfs`, `vfat`, `exfat`,
`btrfs`, `swap`); an unlisted type names no binary. Partitioning a **mounted**
disk is refused rather than forced, matching the unmount policy: the operator
unmounts first, and the panel never guesses.

The read side, `PartitionList`, is available whenever `[volumes] control` is
on, and reports the switches so a client can show which actions are possible.
It is not gated by `disk_allow`, because reading a table changes nothing.

## Error kinds

Failures carry a machine-readable kind, which the HTTP layer maps to a status:

| Kind | Meaning | HTTP |
| --- | --- | --- |
| `disabled` | `[volumes] control` is off | 403 |
| `protected` | Refused by a guard (root, virtual fs, mount root, panel paths, removable) | 403 |
| `permission_denied` | The command could not elevate | 403 |
| `not_found` | Unknown device, missing target, not a mountpoint | 404 |
| `busy` | `umount` reported the target as busy, or the disk has a mounted partition | 409 |
| `invalid` | Bad path, bad type, already mounted | 400 |
| `failed` | Anything else | 500 |

`mount` and `umount` report most failures as plain text on stderr, so the kind
is inferred from that text. The message is passed through verbatim; it is the
only diagnostic the kernel gives.

## Commands

The daemon runs `lsblk` and `/proc/self/mountinfo` on a blocking worker, and
`mount`/`umount` on a separate blocking worker, so a slow filesystem cannot
stall the async connection loop.

## API and CLI

The routes are `GET /api/volumes`, `POST /api/volumes/mount`,
`POST /api/volumes/unmount`, `GET /api/volumes/partitions`,
`POST /api/volumes/partitions` and `POST /api/volumes/partitions/edit`; see
[api-endpoints.md](api-endpoints.md). The CLI mirrors them as `volume list`,
`volume mount`, `volume unmount`, `volume partitions` and the `volume partition`
tree (`create`, `resize`, `delete`, `format`); see [cli.md](cli.md).

Every mutating subcommand asks for confirmation unless `--yes` is given, and
refuses to run when stdin is not a terminal without it.

There is deliberately **no watch endpoint**. Mounts change when an operator
changes them, not continuously, and a stream would add a polling load for a list
that only the panel itself mutates. The web component refreshes on demand.

## Requirements

- Linux with `/proc/self/mountinfo` and `lsblk` from util-linux.
- The `mount` and `umount` binaries for control, plus the privileges to run
  them.
- `sfdisk` for partition edits and the `mkfs.*` utilities (or `mkswap`) for
  formatting. A missing tool answers `failed` with the tool's own message
  rather than guessing with different flags.
- A `/mnt` (or whatever `mount_root` names) that exists before the first mount,
  since the panel does not create directories.
