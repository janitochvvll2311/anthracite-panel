# Panel

Two projects, one application:

- `backend/` — the `anth-panel` binary. Links the SDK, boots the engine, mounts
  its `/api` router in-process and serves the built shell.
- `frontend/` — the Bun project that builds the shell. App modules run in
  sandboxed iframes, never in the panel origin.

## Run it

```sh
cd panel/frontend
bun install
bun run build

cd ../backend
cargo run -- --listen 127.0.0.1:8090 --config /run/anth/config.toml
```

Open `http://127.0.0.1:8090/`. The debug binary reads `panel/frontend/dist`
from disk, so rebuilding the frontend is enough to refresh the shell. A release
build embeds `dist` and fails without it.

## Exposing it

A loopback panel relies on the OS user for access. Any other bind needs a Basic
credential, because the linked engine API is privileged:

```sh
cargo run -- --listen 0.0.0.0:8090 --config /run/anth/config.toml --token "$SECRET"
```

Open the URL in a browser and use username `operator` with the token as the
password. The shell, API and module assets all sit behind it; only `/health` and
the empty sandboxed frame document are public. Without `--token`, a
non-loopback bind refuses to start: there is no silent unauthenticated mode.
Sessions replace this shared secret later.

## Viewport modes

The shell has four discrete modes: `auto`, `desktop`, `tablet` and `mobile`.
`auto` follows the window (mobile under 700 px, desktop from 1100 px). The
selector sits in the sidebar; the choice is stored in `localStorage`, and
`?viewport=mobile` sets it for one load. On mobile the sidebar is a drawer with
a backdrop, a close button and `Escape` to dismiss.

## Isolation

App modules do not run in the panel origin. The panel fetches the module,
hands the source to a sandboxed frame (`/app-frame.html`, opaque origin), and
proxies only that app's own `/api/apps/{id}/` requests. The frame cannot read
the panel DOM, its cookies or its storage.

The panel and the engine share a process and the Unix socket for `anth`; no
second engine may own the same state tree.
