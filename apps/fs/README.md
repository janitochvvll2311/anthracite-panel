# Files

Files is a native host filesystem service plus the sandboxed panel view. The
demo instance still uses the shared deterministic fixture from `demo/emulator`;
this directory builds the real app.

- `backend/` owns an operator-approved root (`--root`, default `/`) and a private
  trash directory (`--trash`, the app's `{data}/trash`). Every path is
  normalised, parent symlinks are canonicalised and checked against the root,
  and final symlinks are resolved before reads. Removals can move an entry to
  the persistent trash and restore it later.
- `frontend/` is the explorer/editor module. It only calls its own WIT routes.

## Build

```bash
apps/fs/build.sh
# writes apps/fs/dist/fs.anth
```

## Install and approve

The native service declares `class = "host"`, so approval needs the full risk
ladder:

```bash
ANTH_SOCKET=/path/to/engine.sock
./target/debug/anth --socket "$ANTH_SOCKET" app install apps/fs/dist/fs.anth
./target/debug/anth --socket "$ANTH_SOCKET" app approve org.anthracite.fs \
    --accept-write --accept-admin --accept-host
```

The approved argv freezes the root; change `--root` in the build script before
building if the app should only see a subtree. It replaces the emulator bundle
under the same app id.
