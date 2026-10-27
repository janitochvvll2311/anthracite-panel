# Wire protocol changelog

Status: pre-1.0.0. The wire is allowed to break; this file records each break
so an operator can tell which side to update.

| Version | Release | State | Notes |
| --- | --- | --- | --- |
| 3 | v0.3.0 (draft) | current | Firewall gained `input`/`output` chains, editable rules, per-chain policies and `firewall_update`/`firewall_set_policy`; `firewall_flush` now carries `{ "chain": "input" or "output" }` |
| 2 | v0.3.0 (draft) | superseded | 4-byte length-prefixed JSON, mandatory `Hello`/`Welcome`, pagination, files/trash/archive/terminal/process/network/firewall operations, one `WireError` |
| 1 | v0.2.0 | superseded | length-prefixed JSON, `Hello`, per-feature wire types |
| unversioned | v0.1.0 | legacy | newline-delimited JSON, 1 MiB line cap, `type`/`data` envelope |

Version 2 gained files, terminal, process, network and firewall operations during the rewrite.
Those are additive variants: no existing frame changed shape, and a peer that
does not know a variant fails loudly at that frame instead of misreading it, so
that version stayed 2.

Version 3 changes the firewall model. `firewall_flush` is no longer a unit
variant and requires a chain, `firewall_add` and `firewall_update` carry a
chain, snapshots gained `chains` with per-chain policies, and
`firewall_update`/`firewall_set_policy` are new. An older CLI must be upgraded
with the daemon. The `volumes`, `volumes_mount` and `volumes_unmount` requests
and their responses are additive to version 3.

## Rules before 1.0.0

- A breaking change bumps `PROTOCOL_VERSION` and is recorded here with the old
  version, the new version and the client action (upgrade, re-login, migrate).
- `anth-wire`, the CLI, the HTTP adapter, the frontend and the docs change
  together. A client and daemon from different versions fail at the handshake
  with both versions named.
- The `features` list in `Hello`/`Welcome` is reserved. It is empty today; an
  unknown feature is ignored so a later version can negotiate without changing
  the handshake.
- Config, plugin manifests and grants are versioned separately. A wire bump does
  not silently reinterpret them.

## At 1.0.0

Freeze the compatibility policy: minimum supported version, deprecation window,
feature-negotiation semantics and migration guarantees. Until then, none of those
are promises.
