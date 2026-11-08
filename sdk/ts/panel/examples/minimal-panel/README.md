# Second panel

A deliberately small panel written against `@anthracite/panel` only. It is the
M3 acid test: it implements `panel.view`, renders whatever app contributes a
view, and can detach a view into a floating pane. It does not import the official
panel or any app.

## Run

```sh
bun install
bun run start
```

It listens on `0.0.0.0:8091` and proxies `/api` to an existing panel/engine at
`http://127.0.0.1:8090` (the official panel). It logs in to that target and
forwards the session cookie; the demo gate below is only this example's own
Basic prompt.

Environment:

- `PORT` — default `8091`
- `ANTH_PANEL` — API transport, default `http://127.0.0.1:8090`; it must be
  loopback or the process refuses to start, because the proxy carries the
  operator's session.
- `ANTH_PANEL_ALLOW_REMOTE=1` — explicit opt-in for a non-loopback transport.
- `ANTH_PANEL_OPERATOR` — operator name to log in with, default `operator`
- `ANTH_PANEL_PASSWORD` — required; read it from the target's
  `state/credentials.txt` (or use the password you changed it to)
- `ANTH_PANEL_AUTH` — `operator:token` for this example's own Basic gate,
  default `operator:anthracite-dev`

Open `http://127.0.0.1:8091/` or the host's VPN address; use `operator` and the
token. The sidebar lists `panel.view` contributions, each opens in a tab, and
**Detach** moves it to a floating pane. Action and settings contributions are
reported as ignored because this panel only implements `panel.view`.
