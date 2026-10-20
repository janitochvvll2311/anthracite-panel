# Volume manager: scope and architecture

Status: approved. Scope decisions below are settled; implementation is in
progress on `feat/volume-manager`.

## 1. What a "volume" means here

A volume is a storage unit the panel can *see* and, when explicitly enabled,
*mount or unmount*. Two different things qualify, and they do not come from the
same source:

| Concept | What it is | Source |
| --- | --- | --- |
| **Device** | A block device: disk, partition, LUKS mapper, LVM LV, MD array, loop, CD-ROM | `lsblk -J` |
| **Mount** | Anything currently mounted: ext4, xfs, btrfs, tmpfs, overlay, NFS, CIFS | `/proc/self/mountinfo` |

They are deliberately kept separate. `lsblk` never reports `overlay` or `tmpfs`
in a container even though those are mounted, and `mountinfo` has no notion of a
partition that is present but unmounted. A single merged list would have to
guess, so the snapshot exposes both and links them by source path.

Virtual filesystems (`proc`, `sysfs`, `cgroup2`, `devpts`, `mqueue`) are
reported but flagged and never eligible for unmount.

## 2. Scope

### In scope for v1

- Read-only inventory: devices with topology, size, filesystem, label, UUID,
  read-only and removable flags, model and transport.
- Mount inventory with usage (`total`/`used`/`available`) and options.
- `mount` a known device on a validated target directory.
- `unmount` a validated mountpoint.
- CLI table, HTTP routes, Lit component, SSE is optional (see §9).

### Explicitly out of scope for v1

`mkfs`, partitioning (`sfdisk`/`parted`), resize, `wipefs`, LVM/RAID creation,
encryption, `fstab` editing, and SMB/NFS credentials.

Two reasons, both concrete:

1. Every one of those is **irreversible from the panel**. A mount mistake is
   undone with one command; a `mkfs` mistake is data loss.
2. The panel still has **no authentication**. `docs/terminal.md` and
   `docs/firewall.md` already carry that warning; adding `mkfs` would turn an
   unauthenticated HTTP port into a destroyer of disks.

Mount/unmount are the only mutations because they are reversible and the
damage is bounded by the guard list in §6.

## 3. Data flow

```text
CLI            HTTP                     Lit component
 |              |                            |
 volume list    GET  /api/volumes           VolumeClient
 volume mount   POST /api/volumes/mount          |
 volume unmount POST /api/volumes/unmount        |
 |              |                                |
 +--------------+--------------------------------+
                |
        anth_proto::Request / Response
                |
        anth_panel::daemon (dispatch + policy)
                |
        anth_panel::volumes
         ├── LsblkRunner  (trait; SystemLsblk in prod, scripts in tests)
         ├── parse_lsblk  (pure: JSON text → Vec<VolumeDevice>)
         ├── parse_mountinfo (pure: mountinfo text → Vec<MountInfo>)
         ├── FsUsage      (trait; statvfs in prod)
         ├── guards (pure, no I/O)
         └── mount / umount via the shared command runner
```

The daemon owns the policy. The wire types never carry "control is enabled";
that is deployment state, exactly as `network`, `firewall` and `files` do today.

## 4. Wire protocol (`anth-proto/src/volumes.rs`)

```rust
pub enum VolumeKind { Disk, Part, Crypt, Lvm, Raid, Loop, Rom, Other }

pub struct VolumeDevice {
    pub name: String,              // sda1, dm-0, nvme0n1p2 — stable id
    pub path: String,              // /dev/sda1
    pub kind: VolumeKind,
    pub parent: Option<String>,    // sda1 -> sda; the UI builds the tree
    pub size_bytes: u64,
    pub fstype: Option<String>,
    pub label: Option<String>,
    pub uuid: Option<String>,
    pub mountpoints: Vec<String>,  // empty when unmounted
    pub read_only: bool,
    pub removable: bool,
    pub model: Option<String>,
    pub transport: Option<String>,
}

pub struct VolumeMount {
    pub mountpoint: String,
    pub source: String,            // /dev/sda1, overlay, server:/export
    pub fstype: String,
    pub options: String,
    pub read_only: bool,
    pub virtual_fs: bool,          // never unmountable
    pub usage: Option<VolumeUsage>,
}

pub struct VolumeUsage { pub total_bytes: u64, pub used_bytes: u64, pub available_bytes: u64 }

pub struct VolumeSnapshot {
    pub devices: Vec<VolumeDevice>,
    pub mounts: Vec<VolumeMount>,
    pub control_enabled: bool,
    pub timestamp_ms: u64,
    pub total: usize,
    pub truncated: bool,
    pub warning: Option<String>,
}

pub enum VolumeErrorKind { Disabled, Protected, NotFound, Busy, PermissionDenied, Invalid, Failed }
pub struct VolumeError { pub kind: VolumeErrorKind, pub detail: String }
```

Requests: `VolumeList`, `VolumeMount { device, target, fs_type: Option<String>, read_only: bool }`,
`VolumeUnmount { target }`.

Responses: `VolumeSnapshot(VolumeSnapshot)`, `VolumeChanged { message }`,
`VolumeError(VolumeError)`.

A flat `devices` list with `parent` mirrors the process monitor decision: the
tree is a presentation concern, so the protocol stays cheap to truncate and the
frontend can render flat or nested from the same payload.

`fit_snapshot` copies the `process.rs`/`firewall.rs` pattern against
`MAX_LINE_LEN`: drop `warning`-worthy detail first (models, serials), then the
least relevant devices, setting `total`, `truncated` and `warning`. Mutations
read the unfitted list, so shortening can never hide a device from a guard.

## 5. Backend module (`anth-panel/src/volumes.rs`)

```rust
// read path
pub fn collect(control_enabled: bool) -> VolumeSnapshot;
fn read_devices(runner: &dyn LsblkRunner) -> Result<Vec<VolumeDevice>, VolumeError>;
pub fn parse_lsblk(json: &str) -> Result<Vec<VolumeDevice>, VolumeError>;
pub fn parse_mountinfo(text: &str) -> Vec<MountInfo>;
fn read_mounts(usage: &dyn FsUsage) -> Vec<VolumeMount>;
fn fit_snapshot(snapshot: VolumeSnapshot) -> VolumeSnapshot;

// write path
pub fn mount(control: bool, sudo: bool, request: &MountArgs) -> Result<String, VolumeError>;
pub fn unmount(control: bool, sudo: bool, target: &str) -> Result<String, VolumeError>;
fn validate_mount(devices: &[VolumeDevice], mounts: &[VolumeMount], args: &MountArgs,
                  mount_root: Option<&Path>) -> Result<ValidatedMount, VolumeError>;
fn validate_unmount(mounts: &[VolumeMount], target: &str,
                    protected: &[PathBuf]) -> Result<PathBuf, VolumeError>;
```

Why shell out to `lsblk` instead of reading `/sys/block`:

- `/sys/block` has no filesystem, label or UUID, and resolving
  `dm`/`md`/`loop` stacking by hand is where distro-specific bugs live.
- `lsblk` already reports the tree, `RO`, `RM`, `MODEL`, `TRAN` and `MOUNTPOINTS`
  in one JSON document.
- The repo already accepts this trade-off for `nft`; the same `Runner` trait
  pattern keeps the logic testable without spawning a process.

`parse_lsblk` is a pure function over JSON so tests use fixtures. It must accept
both `"mountpoint": "/"` (old) and `"mountpoints": ["/"]` (new): util-linux
changed that field, and a daemon that only accepts one of them breaks on the
other distro. A missing `lsblk` binary degrades to mounts-only plus a `warning`
instead of failing the request, matching the "unreadable field degrades" rule in
`hostinfo.rs`.

`parse_mountinfo` is pure over `/proc/self/mountinfo` and must unescape the
octal sequences (`\040` space, `\011` tab, `\134` backslash). It is preferred
over `findmnt` because it needs no subprocess and no parsing of localised
output.

Usage comes from `statvfs(3)` on the mountpoint through a thin `unsafe` wrapper
behind the `FsUsage` trait. Doing it with `lsblk`'s `FSUSED`/`FSAVAIL` would tie
the panel to a recent util-linux and would still say nothing about `overlay`.
Unreadable or permission-denied mountpoints yield `usage: None`.

`timestamp_ms` lets the UI show staleness and stays consistent with the network
snapshot.

## 6. Guards

`unmount` is refused unless **all** hold:

1. `[volumes] control = true`.
2. `target` is absolute, normalised, and has no `..` (reuse `files::normalize`,
   then canonicalise symlinks).
3. `target` is a mountpoint currently listed in `mountinfo` — the daemon can
   only unmount what it just observed, never an arbitrary path.
4. `virtual_fs == false`. A fixed deny list covers `proc`, `sysfs`, `devtmpfs`,
   `devpts`, `cgroup`, `cgroup2`, `mqueue`, `debugfs`, `securityfs`, `pstore`.
5. `target` is not `/`, and is not an ancestor of: the socket path, the config
   path, `[files] root`, or the daemon's own executable. Unmounting `/` or the
   parent of the socket would silently kill the panel.

`mount` is refused unless **all** hold:

1. `[volumes] control = true`.
2. `device` matches a device in the fresh snapshot, by `/dev/...` path or
   `name`, and is not already mounted (`mountpoints` empty).
3. `target` exists, is a directory, is not already a mountpoint, and — when
   `[volumes] mount_root` is set (default `/mnt`) — canonicalises to a path
   inside it. This is the same escape-prevention shape as `files.root`.
4. A filesystem type is known: explicit `fs_type`, otherwise `lsblk`, otherwise
   `blkid -o value -s TYPE`. Unknown means "refuse", never "let mount guess".
5. Not `removable` unless `--allow-removable`; a USB unplug mid-write is a
   failure mode the panel should not invite.

`umount` returns `EBUSY` when the filesystem is in use; that maps to
`VolumeErrorKind::Busy` / HTTP `409` and the CLI exits non-zero with the kernel
message. There is no `-l` (lazy) and no `-f` (force) in v1: both hide a real
problem, and lazy unmount silently strands the mount namespace.

## 7. Configuration

```toml
[volumes]
control = false          # mount/unmount disabled by default
mount_root = "/mnt"      # mount targets must live below this; "" disables the check
```

`dev_default()` turns `control` on for debug builds, matching the existing
pattern for `process`, `network`, `firewall` and `files`. Release keeps it off.

`sudo = true` reuses the global switch: `mount`/`umount` run as
`sudo -n mount ...`, with the same "fall back to direct execution" logic the
firewall uses. That logic is currently private to `firewall.rs`; phase 0
extracts it into `anth_panel::command` so `volumes` does not copy it.

## 8. Surfaces

### CLI (`docs/cli.md`)

```text
anth-panel volume list [--watch [INTERVAL]]
anth-panel volume mount <DEVICE> <TARGET> [--type FSTYPE] [--read-only] [--allow-removable] [--yes]
anth-panel volume unmount <TARGET> [--yes]
```

`mount` and `unmount` ask for confirmation unless `--yes`, like `network up` and
`firewall deny`. `render_volume_snapshot` in `render.rs` draws a bordered table
grouped by device tree depth; the mounts section follows with a blank separator.

### HTTP (`docs/api-endpoints.md`)

| Method | Route | Body | Errors |
| --- | --- | --- | --- |
| GET | `/api/volumes` | — | `502` |
| POST | `/api/volumes/mount` | `{device,target,fs_type?,read_only?}` | `400 403 404 409 500` |
| POST | `/api/volumes/unmount` | `{target}` | `400 403 404 409 500` |

Status mapping from `VolumeErrorKind`: `Disabled`/`Protected`/`PermissionDenied`
→ `403`, `NotFound` → `404`, `Busy` → `409`, `Invalid` → `400`, `Failed` → `500`.

### Frontend

`lib/volumes/volume-client.ts` (`get`, `mount`, `unmount`) and
`lib/volumes/display.ts` (size formatting, usage percentage, tree flattening,
sorting). `components/anth-volume-manager.ts` renders the device tree with a
usage bar, a mounts table, and mount/unmount actions that are only shown when
`control_enabled` is true and are confirmed before sending. It depends only on
the injected `VolumeClient`, imports `formatBytes` from `lib/format.ts` and
`responseError` from `lib/http.ts`, and ships a `.test.html` harness registered
in `component-index.test.html`.

## 9. Deferred: SSE

`/api/volumes/watch` would reuse `poll::stream` and `sse::SseStream` unchanged,
so adding it later is cheap. It is deferred because every tick spawns a
`lsblk` process, which is heavier than the `/proc` reads behind the other
watchers. v1 ships `volume list` plus an explicit refresh in the UI. If the
watcher is added, the interval floor stays at `10ms` like the others.

## 10. Tests

- `anth-proto`: serde round-trip for every new request/response; kind and error
  strings are part of the contract.
- `tests/volumes.rs`: fixture-driven `parse_lsblk` (old and new `mountpoint`
  shapes, missing optional fields, numeric strings); `parse_mountinfo` (escaped
  paths, optional fields, `-` separator); guard matrix for mount and unmount
  with a scripted `LsblkRunner` and a fake `FsUsage`; `fit_snapshot` shrinking a
  large fixture under `MAX_LINE_LEN`.
- `tests/api.rs`: `GET /api/volumes`, both `POST` routes through
  `tower::ServiceExt::oneshot`, and the `403`-when-disabled path.
- `tests/daemon.rs`: dispatch of the three requests with a real `UnixListener`.
- `tests/render.rs`: table alignment with multi-byte labels and an empty list.
- Frontend Vitest: client URL/body/error mapping, `display.ts` helpers, and the
  component with a fake client (control hidden when disabled, confirmation
  required, no refresh while a mutation is in flight).

## 11. Delivery phases

| Phase | Content | Commit |
| --- | --- | --- |
| 0 | Extract the sudo/command runner out of `firewall.rs` into `command.rs` | `refactor: share the sudo command runner` |
| 1 | `anth-proto::volumes` types, requests, responses | `feat: add volume wire types` |
| 2 | Read-only backend: parsers, `collect`, `fit_snapshot`, CLI `volume list`, renderer, `GET /api/volumes`, `docs/volumes.md` | `feat: add the read-only volume inventory` |
| 3 | Mount/unmount with guards, `[volumes]` config, CLI confirmations, HTTP routes | `feat: mount and unmount volumes` |
| 4 | Frontend client, display helpers, component, harness, index entry | `feat: add the volume manager component` |
| 5 | Docs: `README.md`, `cli.md`, `api-endpoints.md`, `socket-protocol.md`, `AGENTS.md` | `docs: document volume management` |

Phases 1–3 are independently reviewable and each ends with a green
`cargo test --workspace --target x86_64-unknown-linux-gnu`, `cargo fmt --check`
and `cargo clippy`.

## 12. Risks

| Risk | Mitigation |
| --- | --- |
| `lsblk` absent or ancient | Mounts-only snapshot plus `warning`; accept both `mountpoint` shapes |
| `lsblk` JSON schema drift across distros | Deserialise into permissive `Option` fields; unknown keys ignored |
| No authentication | Mutations off by default; no destructive operations at all in v1 |
| Container without `CAP_SYS_ADMIN` or `/dev` | `PermissionDenied` with an actionable detail; document the capability |
| Symlinked mount target escapes `mount_root` | `canonicalize` before the prefix check, same as `files.rs` |
| Unmounting the panel's own filesystem | Ancestor guard over socket, config, `files.root` and the executable |
| Busy filesystem | Report `EBUSY` faithfully; no force or lazy flags |

## 13. Decisions

1. **Scope**: partitioning and `mkfs` are deferred. v1 is inventory plus
   reversible mount/unmount only. Revisit once authentication exists.
2. **Target policy**: mount targets must live below `[volumes] mount_root`,
   default `/mnt`. `""` disables the check and is the documented escape hatch.
3. **Mounts coverage**: every mount is shown, including `overlay`, `tmpfs` and
   network filesystems. Virtual filesystems are flagged `virtual_fs` and are
   never unmountable.
4. **Watcher**: no SSE in v1. The UI refreshes on demand; each tick would cost
   one `lsblk` process.
5. **Naming**: `volume`, with `list`/`mount`/`unmount`, matching `file` and
   `network`.
6. **Persistence**: no `/etc/fstab` writes. A bad entry can leave the host
   unbootable, and the panel has no authentication.
