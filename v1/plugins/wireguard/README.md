# WireGuard plugin

The reference plugin: a complete bundle, not a sketch. It shows the whole path a
third-party plugin takes — a Rust guest compiled to `wasm32-unknown-unknown`, a
manifest whose capabilities the host freezes at install, a web module served
from the bundle, and the tests that pin all three.

It manages **persistent** WireGuard interfaces and peers: the panel can create,
edit and delete `/etc/wireguard/<iface>.conf`, and every peer change is written
back to that file. Interface creation, updates, deletion and peer changes are
host-owned jobs; the guest never sends an argv or touches a path.

## Build

The guest is a `cdylib` without WASI. The target is the same one the host is
built for, so no extra toolchain is involved:

```bash
rustup target add wasm32-unknown-unknown
cargo build --target wasm32-unknown-unknown --release
```

That writes `target/wasm32-unknown-unknown/release/anth_plugin_wireguard.wasm`.
The logic tests run on the host and need no target at all:

```bash
cargo test
```

## Bundle

`plugin.toml` is checked in with an all-zero `sha256`. That is deliberate: a
real digest in the repository would be stale the moment the guest changed, and
a stale digest is indistinguishable from a tampered module. `package.sh`
rebuilds the module, computes the digest and writes the zip:

```bash
./package.sh          # writes dist/org.anth.wireguard.zip
```

The integration test `backend/anth-panel/tests/plugin_wireguard.rs` does the
same substitution in a temporary directory, so the shipped manifest is validated
by the real installer without a checked-in artifact.

The script signs the module when `ANTH_PLUGIN_SIGNING_KEY` names an Ed25519
private key. The host verifies `plugin.sig` unless
`[plugins] require_signature = false`:

```bash
openssl genpkey -algorithm ED25519 -out plugin-signing.pem
ANTH_PLUGIN_SIGNING_KEY=plugin-signing.pem ./package.sh
```

The matching public key goes in the host config as 64 hex characters:

```bash
openssl pkey -in plugin-signing.pem -pubout -outform DER \
  | tail -c 32 | od -An -v -tx1 | tr -d ' \n'
```

## Install

```bash
anth-panel plugin install dist/org.anth.wireguard.zip
```

`install` validates every capability against the host allowlist, probes the ABI
and writes the frozen grant to `<config_dir>/plugins.d/org.anth.wireguard.toml`.
It checks `plugin.sig` against `[plugins] public_key`; a missing or mismatching
signature is refused unless the operator set
`[plugins] require_signature = false`, in which case the command prints which
check was skipped.

The plugin needs `wg`, `wg-quick` and `ip`, and permission to write
`/etc/wireguard`. The daemon must run as root or have the relevant capabilities;
release builds default `sudo = false`.

## Capabilities

Read-only views:

| Id | argv |
| --- | --- |
| `wg.interfaces` | `wg show interfaces` |
| `wg.show` | `wg show {interface}` |
| `wg.dump` | `wg show {interface} dump` |

Host jobs (the host owns the argv, paths and secret handling):

| Id | Job | Purpose |
| --- | --- | --- |
| `wg.list` | `wireguard.list` | configured interfaces, addresses, MTU and peers |
| `wg.create` | `wireguard.create` | write the config and bring the interface up |
| `wg.update` | `wireguard.update` | edit interface fields and apply them |
| `wg.delete` | `wireguard.delete` | bring it down and remove the config |
| `wg.add_peer` | `wireguard.add_peer` | append a `[Peer]` block and apply it |
| `wg.remove_peer` | `wireguard.remove_peer` | remove a `[Peer]` block and apply it |

`wg.create` generates the private key with `wg genkey` when the request omits
one. An imported key is written to the `0600` config and is never returned by
the API. `wg.update` preserves the existing private key and peer sections when
those fields are omitted.

## Anchors

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/interfaces` | configured interfaces with addresses, MTU and peers |
| `POST` | `/interfaces` | create one persistent interface |
| `PATCH` | `/interfaces/{interface}` | update one interface |
| `DELETE` | `/interfaces/{interface}` | delete one interface |
| `GET` | `/peers?interface=wg0` | one interface's peers |
| `POST` | `/peers` | add and persist a peer |
| `DELETE` | `/peers` | remove a peer from the config and runtime |
| `GET` | `/status` | counts for a dashboard badge |

They mount under `/api/plugins/org.anth.wireguard/`. The web module derives that
prefix from `import.meta.url` instead of hardcoding it.

Create body:

```json
{
  "interface": "wg0",
  "private_key": "optional; generated when omitted",
  "listen_port": 51820,
  "addresses": ["10.0.0.1/24"],
  "mtu": 1420
}
```

Update body accepts the same fields except `interface`, which comes from the
path. `POST /peers` takes `interface`, `public_key`, `allowed_ips` and an
optional `endpoint`; `DELETE /peers` takes `interface` and `public_key`.

The bundle declares two UI anchors:

- `dashboard.widget` → `anth-wireguard` (compact status)
- `panel.network.tab` → `anth-wireguard-panel`, label `WireGuard`

The second becomes a tab inside the Network theme.

## Security notes

- The guest never sends an argv or a program name. It sends values; the host
  renders the argv from the frozen recipe or the job.
- Interface names, keys, ports, CIDRs and MTUs are validated by the host.
- The config lives under `/etc/wireguard` with mode `0600`; the directory is
  `0700`. Writes are atomic.
- The private key is not in argv, not in logs and not in a job reply.
- A failed create removes the config it just wrote.
- Deleting an interface is confirmed in the UI and removes both the runtime
  link and the config.
