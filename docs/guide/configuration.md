# Configuration

The engine and the panel read the same TOML file. A missing file is created with
the defaults on first boot; `--check` (`anth-engine --check`) validates a file
without opening anything, and `anth doctor` is the CLI wrapper for it.

```toml
version = 1

# The Unix socket the engine listens on. Peer credentials are the identity.
socket = "/run/anth/engine.sock"

# TCP is off unless this is set. Sessions, not peer credentials, apply here.
# listen = "127.0.0.1:8080"

# Everything the engine owns: apps, grants, composition, credentials, keys, data,
# the audit log.
state = "/var/lib/anth"

backend = "cranelift"
```

## Reference

| Key | Default | Meaning |
| --- | --- | --- |
| `version` | — | configuration format; must be `1` |
| `socket` | `/run/anth/engine.sock` | Unix socket path; release builds require an absolute path |
| `listen` | unset (TCP off) | optional `ip:port` for the HTTP surface; sessions apply |
| `state` | `/var/lib/anth` | state tree; release builds require an absolute path |
| `backend` | `cranelift` | the wasm backend; `pulley` is reserved and not built |
| `[log] level` | `info` | `error`, `warn`, `info` or `debug` |
| `[log] format` | `text` | `text` or `json` |
| `[auth] session-ttl` | `12h` | lifetime of a TCP session |
| `[apps] require-signature` | `true` | refuse an unsigned bundle; a release build requires `true` |
| `[apps] allow-unverified-assets` | `false` | development switch; a release build requires `false` |
| `[apps] max-bundle-bytes` | `33554432` | compressed bundle cap |
| `[services] allow-third-party` | release `false`; debug `true` unless declared | may a third-party bundle declare a native service |
| `[limits] fuel` | `10000000` | default per-call fuel |
| `[limits] memory-pages` | `256` | default wasm memory (64 KiB pages) |
| `[limits] wall-ms` | `5000` | default wall-clock timeout |
| `[limits] start-timeout-ms` | `5000` | how long a native service may take to answer `GET /health` |

`[limits]` values are defaults for an app whose manifest omits them; the app's
`[limits]` can lower or raise them within the manifest rules. `fuel`, memory,
the wall clock and stream byte budgets are the four resources an app can spend.

## Profiles

The profile is a property of the **binary**, not of the file:

- `anth-engine --debug` / `--release` selects it when the build allows; a release
  binary cannot be asked to open the debug profile. The panel uses the profile it
  was built with (`debug_assertions`).
- A **release** build refuses a relative `socket` or `state`, `require-signature
  = false` and `allow-unverified-assets = true`. Those are development
  conveniences, not deployment options.
- A **debug** build turns `[services] allow-third-party` on by default unless the
  file declares it, so the host emulator (a third-party bundle) runs without a
  system key. Declaring `allow-third-party = false` in a debug build keeps it
  closed.

`anth doctor --config <path> --debug|--release` runs the same validation offline
and exits with the engine's diagnostics.

## The state tree

The engine owns one directory, created `0700` on first boot:

```
$state/
  apps/<app-id>/          verified bundle + descriptor.toml (engine-written)
  grants/<app-id>.toml    the frozen authority
  keys/<fingerprint>.toml the third-party keyring
  data/<app-id>/          the app's {data}, 0700
  shared/                 the service trust domain ({shared})
  run/                    native service sockets
  composition.toml        the composed profiles
  credentials.txt         the operator password and the first-boot flag
  audit.jsonl             one JSON object per line, append-only
```

Directories are `0700` and the credentials are `0600`; the audit log is `0644`
so a log shipper can read it. The engine refuses a symlinked or
wrong-mode state root rather than following it. The `apps/` tree is not a
scratch area: editing it breaks the grant, and the engine reports the app as a
problem.

## Environment

| Variable | Used by | Meaning |
| --- | --- | --- |
| `ANTH_SOCKET` | `anth` | default socket when `--socket` is absent |
| `ANTH_ENGINE` | `anth doctor` | the engine binary used for the offline check |
| `ANTH_PANEL_WEB` | debug panel | the frontend directory (default `panel/frontend/dist`) |
| `ANTHRACITE_DISTRIBUTION_KEY` | build of `anth-compose` | public key compiled into the binary; not runtime configuration |
| `ANTHRACITE_SIGNING_KEY` | `scripts/package-app.sh` | private key used to sign a staged bundle |
| `ANTHRACITE_LISTEN`, `ANTHRACITE_DEMO_TTL`, `ANTHRACITE_POW_*`, `ANTHRACITE_TRUST_PROXY` | demo variant `run.sh` | demo launcher settings |

`ANTHRACITE_DISTRIBUTION_KEY` is read at compile time. Setting it at runtime has
no effect; a key is trusted because the binary was built with it, not because a
file says so.

## Process flags

`anth-engine`:

| Flag | Meaning |
| --- | --- |
| `--config <path>` | configuration file (default `config.toml`) |
| `--debug` / `--release` | choose the profile when the build allows |
| `--check` | validate configuration, state and composition, then exit |
| `--reset-credentials` | delete the recovery file so the next boot mints a password |

`anth-panel`:

| Flag | Meaning |
| --- | --- |
| `--config <path>` | configuration file (default `config.toml`) |
| `--listen <addr>` | HTTP address (default `127.0.0.1:8090`) |
| `--allow-remote` | required for a non-loopback listen |
| `--reset-credentials` | same as the engine's flag |
| `--web <dir>` | debug builds only: read the frontend from a directory |

Both processes expose `--help`. A change to the file takes effect on restart
(`anth restart`, `systemctl restart`, or stop and start); there is no live
reload of the configuration.

## Secrets

`credentials.txt` holds the operator password and the first-boot flag; it is
`0600` in a `0700` directory. Service secrets live in the app's `{data}`, not in
`anth:store/kv`, and are never returned by a route. The keyring holds public keys
only. The distribution private key is never in the state tree or the repository;
it lives with the release process ([Signing](signing.md)).
