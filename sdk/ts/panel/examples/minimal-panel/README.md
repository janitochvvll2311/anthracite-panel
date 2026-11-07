# Second panel

A deliberately small panel written against `@anthracite/panel` only. It is the
M3 acid test: it implements `panel.view`, renders whatever app contributes a
view, and can detach a view into a floating pane. It does not import the official
panel or any app.

## Run

```sh
bun run start
```

It listens on `0.0.0.0:8091` and proxies `/api` to an existing panel/engine at
`http://127.0.0.1:8090` (the official panel). Both use Basic auth with the same
development token.

Environment:

- `PORT` — default `8091`
- `ANTH_PANEL` — API transport, default `http://127.0.0.1:8090`
- `ANTH_PANEL_AUTH` — `operator:token`, default `operator:anthracite-dev`

Open `http://127.0.0.1:8091/` or the host's VPN address; use `operator` and the
token. The sidebar lists `panel.view` contributions, each opens in a tab, and
**Detach** moves it to a floating pane. Action and settings contributions are
reported as ignored because this panel only implements `panel.view`.
