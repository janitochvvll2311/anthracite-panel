# Storage

Storage is a native host storage service plus the sandboxed panel view. The demo
instance uses the shared scripted fixture from `demo/emulator`; this directory
builds the real app.

- `backend/` lists block devices from `/sys/block` (including partitions) and
  mounts from `/proc/self/mounts`, with usage from `statvfs(3)`. Pseudo
  filesystems and file bind mounts are skipped.
- `attach` validates the block device, an existing mountpoint and the filesystem,
  then runs `mount`. `detach` takes the **mountpoint** (a device can be mounted
  more than once) and runs `umount`; system mounts such as `/`, `/boot` and
  `/boot/efi` are refused.
- Both need `CAP_SYS_ADMIN`; without it the call returns a typed `denied`. The
  privileged host test is deferred in unprivileged environments and the flow is
  exercised through `demo/emulator`.
- `frontend/` is the storage view: mount usage bars and the block-device table.

## Build

```bash
apps/storage/build.sh
# writes apps/storage/dist/storage.anth
```

The script builds and type-checks the frontend, builds `storage-service`, hashes
the binary and module, and packages `manifest.toml`, `wit/storage.wit`,
`web/storage.js` and `bin/storage-service`.

## Install and approve

The real service declares `class = "host"`, so approval needs the full risk
ladder:

```bash
ANTH_SOCKET=/path/to/engine.sock
./target/debug/anth --socket "$ANTH_SOCKET" app upgrade org.anthracite.storage \
    apps/storage/dist/storage.anth
./target/debug/anth --socket "$ANTH_SOCKET" app approve org.anthracite.storage \
    --accept-write --accept-admin --accept-host
```

It replaces the emulator bundle under the same app id. Restart the panel after
the upgrade so the new service starts.
