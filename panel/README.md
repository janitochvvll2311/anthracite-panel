# Panel

Two projects, one application:

- `backend/` — the `anth-panel` binary. Serves the built shell and forwards
  `/api/*` to the engine over its Unix socket, unchanged.
- `frontend/` — the Bun project that builds the shell.

## Run it

```sh
cd panel/frontend
bun install
bun run build

cd ../backend
cargo run -- --listen 0.0.0.0:8090 --engine /run/anth/engine.sock
```

Open `http://<host>:8090/`. The debug binary reads `panel/frontend/dist` from
disk, so rebuilding the frontend is enough to refresh the shell. A release
build embeds `dist` and fails without it.

The panel must run as the same user as the engine: the engine authorizes a
Unix-socket peer by uid. Sessions will replace that constraint.
