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

The panel authenticates the browser with the engine's session cookie. On first
boot the engine generates the operator password in `state/credentials.txt`
(`0600`); open `/setup`, read that file and change it. Later visits go through
`/login`.

The shell, API and module assets all require a session; only `/health`, `/login`,
`/setup` and the empty sandboxed frame document are public. There is no shared
Basic secret and no silent unauthenticated mode.

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
