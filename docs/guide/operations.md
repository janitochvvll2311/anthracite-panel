# Operations

## Processes

One process owns the engine: `anth-panel` (the official panel) or `anth-engine`
(headless). Do not run both against the same state and socket. The panel links
the engine; its HTTP shell and the engine API are the same process.

- **Liveness:** `GET /health` is unauthenticated and answers when the process is
  serving.
- **Readiness:** `GET /api/health` (authenticated) answers `starting` while the
  daemon boots apps, then `ready` or `degraded`; `anth status` is the CLI form.
- A fatal composition error still serves the API so the operator can repair it;
  readiness tells the difference between "starting", "degraded" and "cannot
  resolve".

## Logs and audit

The engine logs to stderr with `[log] level` (`error`, `warn`, `info`, `debug`)
and `[log] format` (`text` or `json`). The panel writes the engine log and its
own access log to the same place when run from a terminal; a service manager
captures it.

The **audit log** is the durable record: `$state/audit.jsonl`, one JSON object per
line, append-only. Actions are `app.install`, `app.upgrade`, `app.approve`,
`app.revoke`, `app.enable`, `app.disable`, `app.remove`, `compose`,
`engine.restart`, `key.add`, `key.remove`, `acknowledge`, `deny`,
`session.login`, `service.start`, `service.stop`, `service.crash`,
`service.deny`. Each entry has `at`, `actor`, `action`, `app`, `result` and a
sanitized `detail`; denials are recorded even when nothing changed.

Read it with `anth audit [--limit n] [--before cursor]` or `GET
/api/engine/audit`. There is no rotation or retention policy in 0.6: the file
grows until an operator moves it. Ship it to your log system if you need
retention.

## Backup and restore

The state tree is the whole installation: apps, grants, keys, data, the
composition, credentials and the audit log. Back it up as a unit.

1. Stop the panel/engine (or accept a crash-consistent copy; a stopped copy is
   the supported one).
2. Copy `$state` with permissions (`rsync -a`, `tar -cp`). The directory is
   `0700`; a backup that leaks the keyring and the credentials is a backup of the
   operator's authority.
3. Restore it to the same path on the same or a new host, then start the panel.

The distribution private key is **not** in the state tree; it belongs to the
release process. A restored state keeps working with a new binary as long as the
binary trusts the same signers; a release key rotation requires re-signing and
reinstalling the system bundles.

## Restart and configuration changes

Configuration is read at boot. A change takes effect on restart:

```sh
anth restart                  # re-exec the process that owns the engine
systemctl restart anthracite  # or the service manager
```

`anth restart` answers when the request is accepted (`202`), not when the app is
ready; poll `anth status`. The pid survives a re-exec, and native services are
stopped and started with the process. `compose --now` saves the document and asks
for that restart.

## Upgrades

1. Back up the state.
2. Read the release notes and verify the artifacts (`sha256sum -c`, `anth
   --version`, `anth key distribution`).
3. Stop the process, replace the binaries (and the signed bundles for a release),
   start it again.
4. Check `anth status` and `anth audit --limit 20`; a signed build refuses a
   system bundle it did not sign, so a key rotation is a release, not a patch.

State written by 0.6.x is read by later 0.6.x binaries; a downgrade is not
supported if a newer binary wrote a format the older one does not know. Keep the
backup from step 1 until the new version is verified.

## Resource limits

Per call, an app spends fuel, wasm memory, wall time and stream bytes
(`[limits]` defaults; the manifest can set them). A native service has a start
timeout and a restart budget: three restarts inside sixty seconds, then the app
is degraded and not retried. A stream over budget ends with `exhausted`. These
are enforced by the engine; the OS limits are the L0 baseline described in
[Security](security.md#services-and-the-sandbox).

## Monitoring

- `GET /health` for the process; `GET /api/health` or `anth status` for readiness.
- `GET /api/engine/composition`'s `surface.degraded` and `surface.fatal` for the
  composition state.
- `anth audit` for the security-relevant history; `deny`, `service.deny` and
  `service.crash` are the entries to alert on.
- Disk: the state grows with apps, data and the audit log. The engine refuses a
  wrong-mode state root; keep it on a filesystem with room for bundles and
  service data.

## One instance, one state

A state directory belongs to one engine instance. Two processes on the same
state will fight over sockets, grants and the audit log; use a second state and a
second socket for a second instance (the demo variants do exactly that per
session). The default paths are `/run/anth/engine.sock` and `/var/lib/anth`.

## Removing an app

`anth app remove <id>` uninstalls the bundle but keeps `$state/data/<id>` and
anything the app published under `$state/shared`. Removing those is an explicit
operator cleanup: they may be the only copy of a certificate, a database or a
service's state. Check the grant and the data before deleting.
