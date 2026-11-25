# Installation

Anthracite ships as a set of static binaries plus signed bundles, or as four
release variants that package them together. There is no installer: the engine
and the panel are ordinary processes.

## Requirements

- Linux on x86_64. The release binaries are statically linked (musl, PIE) and
  need no libc on the host.
- A short state path. Service sockets live under the state directory and Linux
  caps a Unix socket path at 107 bytes; a deeply nested path degrades every
  native service.
- No root for `core`, `demo` or `demo-sessions`; the real system apps in `base`
  need root for host reach, and `net`/`firewall` need `CAP_NET_ADMIN` (and `nft`
  for the firewall).
- From source: Rust 1.99.1, the `wasm32-unknown-unknown` target, Bun 1.4.4 and
  `wasm-tools`. The root `.cargo/config.toml` shares one target directory.

## Release variants

| Variant | Contains | Start |
| --- | --- | --- |
| `core` | `anth`, `anth-engine`, `anth-panel`; empty composition | `./run.sh` |
| `base` | core + the eight signed system bundles | `./run.sh` installs, approves and composes |
| `demo` | core + the eight emulated system bundles, one state per run | `./run.sh` |
| `demo-sessions` | demo with one ephemeral, Turnstile-gated session per browser | `./run.sh` (landing page on 8091) |

Each archive has `bin/`, `run.sh`, `README.md`, `manifest.json` and
`SHA256SUMS`; `base`, `demo` and `demo-sessions` also have `bundles/`. Verify
with `sha256sum -c SHA256SUMS`, check the version (`./bin/anth --version`) and
the key the binary trusts (`./bin/anth --socket state/engine.sock key distribution`
while it runs). The manifest records the key fingerprint and every digest.

The variants listen on `127.0.0.1:8090` (`demo-sessions` on `127.0.0.1:8091`).
`run.sh --listen <addr>` changes it; `ANTHRACITE_LISTEN` does the same. State is
`state/` (or a `state/session-*` for the demo). The first boot writes the
operator password to the state directory's `credentials.txt`.

## Docker images

The four variants are also published as Docker-compatible images
(`docker load -i <file>.tar.gz`), with a minimal userspace, the binaries under
`/opt/anthracite` and a state volume:

| Image | User | Ports | Notes |
| --- | --- | --- | --- |
| `anthracite-core:0.7.0` | 1000 | 8090 | no apps |
| `anthracite-base:0.7.0` | root | 8090 | real services; `nft` is not in the image |
| `anthracite-demo:0.7.0` | 1000 | 8090 | the eight emulated system apps |
| `anthracite-demo-sessions:0.7.0` | 1000 | 8091, 8100-8199 | Turnstile variables required |

```sh
docker load -i anthracite-0.7.0-core.tar.gz
docker run -p 8090:8090 -v anth-core:/opt/anthracite/state anthracite-core:0.7.0

docker run -p 8091:8091 -p 8100-8199:8100-8199 \
  -v anth-sessions:/opt/anthracite/state \
  -e ANTHRACITE_CAPTCHA_SITE_KEY -e ANTHRACITE_CAPTCHA_SECRET \
  -e ANTHRACITE_CAPTCHA_HOSTNAME -e ANTHRACITE_TRUST_PROXY=1 \
  anthracite-demo-sessions:0.7.0
```

`base` may need `--cap-add=NET_ADMIN` and an `nft` binary. A bind-mounted state
directory must be writable by uid 1000 (or root for `base`). See [Demo](demo.md)
for the session launcher's proxy and range details.

## Single bundles

The release also publishes every app on its own, so an existing deployment can
take one app without downloading another variant:

| Asset | Contents |
| --- | --- |
| `system-org.anthracite.<app>.anth` | the signed real system service, the same bundle `base` carries |
| `demo-org.anthracite.<app>.anth` | the signed host emulator under the same id |
| `anthracite-0.7.0-bundles.tar.gz` | every signed system and emulator bundle with its `SHA256SUMS` |

```sh
sha256sum -c SHA256SUMS
anth app install system-org.anthracite.fs.anth
anth app show org.anthracite.fs        # the risk, the interfaces, the grant state
anth app approve org.anthracite.fs --accept-write --accept-admin --accept-host
anth compose --add org.anthracite.fs --now
```

The flags depend on the risk the approval preview computes; `--accept-host` is
only needed for a service that declares a host-reaching class. A `system` and a
`demo` bundle share the app id, so install one or the other, not both: the second
install is a `conflict`. Every bundle in the release is signed by the same
distribution key, so a keyed binary verifies them without adding anything to the
keyring.

## From source

```sh
(cd panel/frontend && bun install && bun run build)       # the release panel embeds this
cargo build --release --manifest-path panel/backend/Cargo.toml
cargo build --manifest-path engine/Cargo.toml -p anth-engine
cargo build --manifest-path sdk/rust/Cargo.toml \
  -p anth-cli -p anth-signer -p anth-conformance
```

`cargo build` without `--release` builds the debug panel, which reads the
frontend from disk (`--web`, default `panel/frontend/dist`). A **release** panel
fails to build without `panel/frontend/dist`, by design: the shell is embedded.

Verification commands used by the project:

```sh
(cd engine && cargo test --workspace)
(cd sdk/rust && cargo test --workspace)
(cd panel/backend && cargo test --workspace)
cargo fmt --all --check                      # inside each workspace
cargo clippy --workspace --all-targets -- -D warnings
bun scripts/check-edges.mjs                  # the forbidden edges
scripts/acid-test.sh                         # the external-bundle path
```

## Configuration and first boot

A minimal configuration:

```toml
version = 1
socket = "/run/anth/engine.sock"
state = "/var/lib/anth"

[apps]
require-signature = true
```

Run the panel:

```sh
anth-panel --config /etc/anthracite/config.toml --listen 127.0.0.1:8090
```

A **release** build requires absolute `socket` and `state` paths,
`require-signature = true` and `allow-unverified-assets = false`. A debug build
may use relative paths and the development switches. The first boot creates the
state tree and `credentials.txt`; sign in as `operator` and change the password.
The full file is in [Configuration](configuration.md).

## Run it as a service

The panel is the process that owns the engine; run one of them, not both. A
systemd unit for a loopback panel:

```ini
[Unit]
Description=Anthracite panel
After=network.target

[Service]
Type=simple
User=anthracite
Group=anthracite
RuntimeDirectory=anth
StateDirectory=anth
ExecStart=/usr/local/bin/anth-panel --config /etc/anthracite/config.toml --listen 127.0.0.1:8090
Restart=on-failure
RestartSec=2

[Install]
WantedBy=multi-user.target
```

`RuntimeDirectory=anth` provides `/run/anth` for the socket; `StateDirectory=anth`
provides `/var/lib/anth` with the right ownership. Put TLS and the public
hostname in front of the panel; `--listen` on a non-loopback address requires
`--allow-remote` and is an explicit decision.

The headless engine (`anth-engine`) uses the same configuration and is the right
choice when no panel is needed. It binds the Unix socket and, if `[listen]` is
set, TCP.

## Upgrades and removal

1. Back up the state directory (see [Operations](operations.md#backup-and-restore)).
2. Stop the panel or engine.
3. Replace the binaries and, for a release, the signed bundles.
4. Start it again; the state tree and the composition are read as they are.

A release key rotation is a new release: the new binary trusts the new key and
the system bundles must be signed again and reinstalled. Third-party bundles stay
valid as long as their signer is in the keyring.

To remove an installation: stop the process, remove the binaries and the
configuration, and remove the state directory only if you also want to lose the
apps, grants, credentials and audit log. Uninstalling a single app is a different
operation (`anth app remove`); it never deletes the app's `{data}` on its own.
