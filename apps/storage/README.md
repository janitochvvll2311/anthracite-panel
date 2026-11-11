# Storage

Storage is a native read-only host storage service plus the sandboxed panel
view. The demo instance uses the shared scripted fixture from `demo/emulator`;
this directory builds the real app.

- `backend/` lists block devices from `/sys/block` (including partitions) and
  mounts from `/proc/self/mounts`, with usage from `statvfs(3)`. Pseudo
  filesystems and file bind mounts are skipped.
- `attach` and `detach` are refused with a typed `denied` error. Mounting and
  unmounting need `CAP_SYS_ADMIN` and are the next approved slice.
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
