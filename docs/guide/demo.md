# Demo

Anthracite has two demo shapes: a **developer demo** that runs the deterministic
host emulator under a debug panel, and the **release demo** variants that package
the emulator as signed bundles behind a strict configuration. Both compose the
**eight system app ids**, exactly like `base`: the difference is who answers them
(`home`, `fs`, `terminal`, `proc`, `net`, `firewall`, `storage` and `accounts`
are emulated, not the host). A demo that ships fewer than the eight is not a
demo build. Neither shape reads the host you run it on; the emulator is an app
like any other.

## The host emulator

`demo/emulator` is one native service binary that answers under the future system
app ids: `org.anthracite.fs`, `terminal`, `proc`, `net`, `firewall`, `storage`,
`accounts` and `home`. Each bundle carries the real app's WIT package and id, so
replacing the emulator with the real service is an upgrade, not a UI change.
Fixtures are deterministic; files, accounts, network and storage are durable in
`{shared}/emulator/host.json`, processes and terminal sessions are runtime.

The emulator is a **third-party** bundle by design: in a debug build the
`allow-third-party` switch is the explicit opt-in, and in the release variants it
is signed by the release key and installs as `system` with a strict
configuration.

## The developer demo

```sh
# Only after editing a frontend; the generated modules are committed.
apps/build-frontends.sh

cargo build --manifest-path demo/emulator/Cargo.toml --release
cargo run --manifest-path demo/emulator/Cargo.toml --release --bin bundle-emulator -- \
  --binary target/release/host-emulator --out demo/dist

scripts/dev.sh
```

`scripts/dev.sh` serves the developer demo on **127.0.0.1:5173** (override with
`ANTH_DEV_LISTEN`, and the config with `ANTH_DEV_CONFIG`); 5173 is the dev port
throughout the guide. A preview proxy needs a non-loopback address:

```sh
ANTH_DEV_LISTEN=0.0.0.0:5173 scripts/dev.sh
```

That is the only case where the script passes `--allow-remote`; the demo config
disables signature checks, so the loopback default is deliberate. The script
rebuilds `panel/frontend/dist` when a source is newer than the served module,
because the debug backend reads the shell from disk.

`demo/config.toml` turns signature checks and the third-party service switch on
for development; it is a **debug** configuration and a release build refuses it.
Install and compose the eight bundles with the CLI (the exact commands are in
[`demo/README.md`](../../demo/README.md)): install each `demo/dist/<app>.anth`,
approve with the levels its class needs, then compose and restart. The state is
`demo/state`; every browser that opens the panel sees the same demo, which is
what makes it a developer fixture rather than a public service.

## The release demo

The `demo` variant of the release is the same emulator, signed by the release
key and installed by `run.sh` under a strict configuration
(`require-signature = true`, `allow-third-party = false`). Each run gets its own
state directory:

```sh
./run.sh                       # a new state/session-*; 127.0.0.1:8090
./run.sh --state my-demo       # resume a previous session
./run.sh --ephemeral           # delete the state on exit
```

The operator password is generated in that state (`credentials.txt`). The
emulator snapshot lives at `<state>/shared/emulator/host.json`; a session's
files, users, routes and mounts persist until you delete the directory or use
`--ephemeral`.

## The per-session demo

`demo-sessions` is the public shape: one panel and one emulator state **per
browser session**, no operator password, and a CAPTCHA gate.

```sh
ANTHRACITE_CAPTCHA_SITE_KEY=<site key> \
ANTHRACITE_CAPTCHA_SECRET=<secret> \
ANTHRACITE_CAPTCHA_HOSTNAME=demo.example \
ANTHRACITE_TRUST_PROXY=1 \
./run.sh --listen 0.0.0.0:8091 --public-host demo.example
```

- `GET /` shows a landing page with **Cloudflare Turnstile**. The token is
  verified server-side for `success`, `action = demo-session` and the configured
  hostname; `Origin`/`Referer` is checked too. A non-loopback launcher refuses to
  start without Turnstile, the site key, the secret and the hostname.
- `POST /start` creates a panel on its own port with a fresh state, changes the
  panel's generated password to a random secret, logs the visitor in through the
  engine's session endpoint and forwards the session cookie. The visitor never
  types a password.
- Sessions expire after `--ttl-seconds` (30 minutes by default); the launcher
  kills the panel, removes its state and forgets the cookie mapping. The live
  session count is capped (`--max`, 8 default).
- Abuse limits: three sessions per address per hour, six per minute and sixty per
  hour globally, ten attempts per address per minute, and a one-hour block after
  five failed challenges. Per-address limits use `CF-Connecting-IP` or
  `X-Forwarded-For` only with `--trust-proxy`; the operator's proxy must overwrite
  those headers.
- Child panels use an ephemeral port by default. `--port-base`/`--port-count`
  (or `ANTHRACITE_PORT_BASE`/`ANTHRACITE_PORT_COUNT`) pin the range for a
  container or a proxy. The image exposes 8091 and 8100-8199.

The launcher binds loopback by default and does not proxy. The demo runs on its
own domain, behind the operator's TLS proxy, and is **not** embeddable: the
panel keeps `frame-ancestors 'none'`. Publish the launcher and the child range
through the proxy; do not expose the child ports directly. The visitor's engine
cookie is host-scoped, so one browser maps to one session at a time.

### Docker image

```sh
docker run -p 8091:8091 -p 8100-8199:8100-8199 \
  -v anth-sessions:/opt/anthracite/state \
  -e ANTHRACITE_CAPTCHA_SITE_KEY -e ANTHRACITE_CAPTCHA_SECRET \
  -e ANTHRACITE_CAPTCHA_HOSTNAME -e ANTHRACITE_TRUST_PROXY=1 \
  anthracite-demo-sessions:0.6.1
```

The image runs as uid 1000, with the state at `/opt/anthracite/state` and
`ANTHRACITE_PORT_BASE=8100` already set. The `demo` image (one state per run) is
the simpler one if you do not need per-browser isolation.

## Verifying a demo

```sh
sha256sum -c SHA256SUMS
bin/anth --version
bin/anth --socket state/engine.sock key distribution
bin/anth --socket state/engine.sock app list          # eight system apps
bin/anth --socket state/engine.sock app home info      # the fixture, not the host
```

`app home info` is the quickest proof that the emulator is answering: the CPU,
memory and disks are fixtures with the demo's model name, not the machine.

## Reset and cleanup

- A developer demo: stop the panel, remove `demo/state`, start again.
- A release demo run: use `--ephemeral`, or delete `state/session-*`.
- Per-session demo: sessions clean themselves on expiry. A state directory is a
  complete engine tree (`state/sessions/<id>/state`), so an operator can archive
  or delete one; keep the root short because of the Unix socket path limit (107
  bytes).
