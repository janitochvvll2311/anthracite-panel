# WireGuard plugin

The reference plugin: a complete bundle, not a sketch. It shows the whole path a
third-party plugin takes — a Rust guest compiled to `wasm32-unknown-unknown`, a
manifest whose capabilities the host freezes at install, a web module served
from the bundle, and the tests that pin all three.

It manages interfaces and peers; it does not create tunnels. `wg-quick` and the
`/etc/wireguard` files stay the operator's, which is why the plugin needs only
`wg show` and `wg set`.

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

## Install

```bash
anth-panel plugin install dist/org.anth.wireguard.zip
```

`install` validates every capability against the host allowlist, probes the ABI
and freezes the recipes in the config. It warns that the bundle is unsigned.
`wg.add_peer_endpoint` carries free text for the endpoint, so the installer
warns about that field specifically.

## Capabilities

| Id | argv |
| --- | --- |
| `wg.interfaces` | `wg show interfaces` |
| `wg.show` | `wg show {interface}` |
| `wg.dump` | `wg show {interface} dump` |
| `wg.add_peer` | `wg set {interface} peer {public_key} allowed-ips {allowed_ips}` |
| `wg.add_peer_endpoint` | the same, plus `endpoint {endpoint}` |
| `wg.remove_peer` | `wg set {interface} peer {public_key} remove` |

The last two exist as separate recipes rather than one with an optional
trailing argument. `wg` takes `endpoint` and its value as two argv elements, and
the host drops whole elements, so an omitted value would leave a bare `endpoint`
behind. One capability per shape keeps every approved command complete.

## Anchors

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/interfaces` | every interface with its peers |
| `GET` | `/peers?interface=wg0` | one interface's peers |
| `POST` | `/peers` | add a peer |
| `GET` | `/status` | counts, for a dashboard badge |

They mount under `/api/plugins/org.anth.wireguard/`. The web module derives that
prefix from `import.meta.url` instead of hardcoding it, so a different mount
point does not need an edit.

`POST /peers` body:

```json
{
  "interface": "wg0",
  "public_key": "yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk=",
  "allowed_ips": ["10.0.0.2/32"],
  "endpoint": "vpn.example.net:51820"
}
```

`endpoint` is optional and selects the `wg.add_peer_endpoint` recipe.

## Security notes

- The guest never sends an argv or a program name. It sends values; the host
  renders the argv from the frozen recipe.
- `public_key` is validated as base64 that decodes to 32 bytes, both by the host
  and by the guest. `allowed_ips` is a comma-separated CIDR list with no spaces,
  because a space would be passed to `wg` verbatim.
- No capability can read a private key: the only `wg show` forms granted are the
  human summary and the dump, and neither is filtered — so granting `wg.show`
  grants what that command prints, which never includes a private key. Keep it
  that way if you add capabilities.
