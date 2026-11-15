# Getting started

This page gets a panel running with an app, first from a release and then from
source. Everything else in the guide assumes that you can do both.

## Option A: a pre-release variant

The release variants carry the panel, the CLI, the engine and signed bundles.
Unpack one and run its `run.sh`; there is no installer and no daemon setup.

| Variant | Apps | State |
| --- | --- | --- |
| `core` | none; an empty composition | `state/` |
| `base` | the eight signed system apps | `state/` |
| `demo` | the eight emulated system apps | a fresh `state/session-*` per run |
| `demo-sessions` | the same eight apps, one ephemeral session per browser | `state/sessions/<id>` |

```sh
tar -xzf anthracite-0.6.1-core-linux-x86_64.tar.gz
cd core
./run.sh                       # http://127.0.0.1:8090
```

The first boot writes the operator password to `state/credentials.txt`. Open the
panel, sign in as `operator` with that password and set a new one. The CLI talks
to the same engine over the Unix socket:

```sh
./bin/anth --socket state/engine.sock status
./bin/anth --socket state/engine.sock key distribution
```

`base` installs and composes the system apps on the first run; `demo` installs
the eight emulator bundles under the same ids and prints the session state path.
`demo-sessions` is the
public demo: it runs a self-hosted proof-of-work challenge, needs its own domain,
and is documented in [Demo](demo.md). The Docker images are the same variants;
see [Installation](installation.md#docker-images).

To verify a download before running it:

```sh
sha256sum -c SHA256SUMS
./bin/anth --version
```

## Option B: from source

Requirements: Linux, Rust 1.99.1 with the `wasm32-unknown-unknown` target, Bun
1.4.4 and `wasm-tools`. The three Rust workspaces share one target directory
through the root `.cargo/config.toml`.

```sh
# The panel embeds the built frontend in a release build; a debug build reads it
# from disk. Build it once either way.
(cd panel/frontend && bun install && bun run build)

cargo build --manifest-path panel/backend/Cargo.toml
cargo build --manifest-path sdk/rust/Cargo.toml -p anth-cli -p anth-signer
```

The debug `anth-panel` is at `target/debug/anth-panel`; the CLI is
`target/debug/anth`. Write a configuration (debug builds accept relative paths;
release builds need absolute ones):

```toml
# /tmp/anth/config.toml
version = 1
socket = "/tmp/anth/engine.sock"
state = "/tmp/anth/state"

[apps]
require-signature = false   # development only; a release build refuses this
```

Run the panel and install the example app:

```sh
mkdir -p /tmp/anth/state
./target/debug/anth-panel --listen 127.0.0.1:8090 --config /tmp/anth/config.toml

# in another terminal
scripts/build-app.sh apps/hello
scripts/package-app.sh apps/hello /tmp/hello.anth
./target/debug/anth --socket /tmp/anth/engine.sock app install /tmp/hello.anth
./target/debug/anth --socket /tmp/anth/engine.sock app approve org.anthracite.hello
./target/debug/anth --socket /tmp/anth/engine.sock compose --add org.anthracite.hello --now
./target/debug/anth --socket /tmp/anth/engine.sock app hello greet --name Ada
```

The first boot created `/tmp/anth/state/credentials.txt`; sign in to
<http://127.0.0.1:8090> with it. The demo instance
([`demo/`](../../demo/README.md)) is the same flow with the emulator bundles.

## What next

- [Configuration](configuration.md) for the full file and the profiles.
- [CLI](cli.md) and [HTTP API](http-api.md) for the two surfaces.
- [Apps](apps.md) and [Signing](signing.md) to build and sign your own bundle.
- [Security](security.md) before you run anything you did not build.
