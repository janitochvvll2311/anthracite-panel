# Wire protocol changelog

Status: pre-1.0.0. The wire is allowed to break; this file records each break
so an operator can tell which side to update.

| Version | Release | State | Notes |
| --- | --- | --- | --- |
| 4 | v0.4.0 (draft) | current | HTTP is the only dialect: no length-prefixed frames and no `Hello`/`Welcome`; `GET /api/hello` plus `X-Anth-Protocol: 4` replaces the handshake; the Unix socket serves the same router with `peer_cred` authentication; the terminal is one WebSocket shared by the browser and the CLI; `anth-wire` is DTO-only |
| 3 | v0.3.0 (draft) | superseded | Firewall gained `input`/`output` chains, editable rules, per-chain policies and `firewall_update`/`firewall_set_policy`; `firewall_flush` now carries `{ "chain": "input" or "output" }` |
| 2 | v0.3.0 (draft) | superseded | 4-byte length-prefixed JSON, mandatory `Hello`/`Welcome`, pagination, files/trash/archive/terminal/process/network/firewall operations, one `WireError` |
| 1 | v0.2.0 | superseded | length-prefixed JSON, `Hello`, per-feature wire types |
| unversioned | v0.1.0 | legacy | newline-delimited JSON, 1 MiB line cap, `type`/`data` envelope |

Version 4 removes the frame transport. `anth-wire` no longer exports `Request`,
`Response`, `read_frame` or `write_frame`; the daemon keeps its internal command
enum, and every client speaks HTTP. The CLI moved from typed frames to
`anth-client` over the Unix socket, `--watch` moved from repeated requests to
SSE, and `files download` streams the HTTP body instead of base64 chunks.
`FileBytes` and the chunked `files_download` operation are gone. A daemon and
client from different versions fail at `/api/hello` or on the `X-Anth-Protocol`
header; the CLI checks both before a command runs.

Version 3 changes the firewall model. `firewall_flush` is no longer a unit
variant and requires a chain, `firewall_add` and `firewall_update` carry a
chain, snapshots gained `chains` with per-chain policies, and
`firewall_update`/`firewall_set_policy` are new. An older CLI must be upgraded
with the daemon. The `volumes`, `volumes_mount`, `volumes_unmount`,
`volume_partition_create`, `volume_partition_resize`,
`volume_partition_delete`, `volume_format`, `accounts`, `account_create`,
`account_delete`, `account_set_password`, `account_group_create` and
`account_group_delete` requests and their responses are additive to version 3.

Version 2 gained files, terminal, process, network and firewall operations
during the rewrite. Those were additive variants: no existing message changed
shape, so that version stayed 2.

## Rules before 1.0.0

- A breaking change bumps `PROTOCOL_VERSION` and is recorded here with the old
  version, the new version and the client action (upgrade, re-login, migrate).
- `anth-wire`, `anth-client`, the CLI, the HTTP adapter, the frontend and the
  docs change together. A client and daemon from different versions fail at
  `/api/hello` with both versions named.
- `anth-wire` carries data transfer objects only. A transport change does not
  add a request enum back to it.
- Config, plugin manifests and grants are versioned separately. A wire bump does
  not silently reinterpret them.

## At 1.0.0

Freeze the compatibility policy: minimum supported version, deprecation window,
feature-negotiation semantics and migration guarantees. Until then, none of those
are promises.
