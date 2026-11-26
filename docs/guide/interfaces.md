# Interface reference (v0.8.0)

The five edge services export one WIT package each. The types below are a
summary; the authoritative record is the `.wit` file in the bundle, and that is
what the engine checks against the grant. All errors use
`anth:error/kinds@1.0.0`.

## `anthracite:certs/store@1.0.0`

`apps/certs/wit/certs.wit`

| Function | Shape |
| --- | --- |
| `inventory` | `() -> list<certificate>` |
| `get-certificate` | `(id) -> option<certificate>` |
| `import-certificate` | `(import-request) -> result<certificate, error>` |
| `store-issued` | `(import-request) -> result<certificate, error>` |
| `remove-certificate` | `(id) -> result<bool, error>` |
| `validate` | `(id) -> validation` |
| `publish` | `(id) -> result<certificate, error>` |
| `set-auto-renew` | `(id, enabled) -> result<certificate, error>` |

`certificate` carries names, issuer, validity, key type, source, SHA-256
fingerprint, status and the renewal flag. `import-request` carries the chain,
the key and the names; it is write-only. `validation` reports `ok`, `chain-ok`
and `key-matches`.

## `anthracite:acme/client@1.0.0`

`apps/acme/wit/acme.wit`

| Function | Shape |
| --- | --- |
| `list-accounts` | `() -> list<account>` |
| `create-account` | `(directory, contact, key-type, eab) -> result<account, error>` |
| `issue` | `(issue-request) -> result<order, error>` |
| `renew` | `(certificate) -> result<order, error>` |
| `revoke` | `(certificate, reason) -> result<bool, error>` |
| `list-orders` | `() -> list<order>` |
| `follow` | `(order) -> result<stream<order-event>, error>` |
| `capabilities` | `() -> list<capability>` |
| `confirm-challenge` | `(order) -> result<order, error>` |

`issue-request` selects the directory, names, challenge, key type and either the
HTTP-01 root or the DNS solver. `order` carries the state, challenge and any
`issued-unstored` warning; account keys and EAB secrets are never in the record.

## `anthracite:proxy/admin@1.0.0`

`apps/proxy/wit/proxy.wit`

| Function | Shape |
| --- | --- |
| `snapshot` | `() -> config` |
| `apply` | `(config, expected-version) -> result<config, error>` |
| `status` | `() -> proxy-status` |
| `logs` | `() -> result<stream<access-event>, error>` |
| `certificates` | `() -> list<string>` |
| `reload-certificates` | `() -> result<proxy-status, error>` |
| `hosts` | `() -> list<host>` |
| `routes` | `() -> list<route>` |
| `test-upstream` | `(id) -> result<upstream-check, error>` |

`config` is the whole document: hosts, listeners, routes, upstreams and header
policy. `apply` rejects a version that is not `expected-version` and keeps the
last good document on any validation failure.

## `anthracite:containers/runtime@1.0.0`

`apps/containers/wit/containers.wit`

| Function | Shape |
| --- | --- |
| `list-containers` | `() -> list<container>` |
| `inspect` | `(id) -> option<container>` |
| `create` | `(create-request) -> result<container, error>` |
| `start` / `stop` / `restart` / `pause` / `unpause` / `kill` / `remove` | lifecycle, `result<bool, error>` |
| `logs` | `(id, tail, follow) -> result<stream<list<u8>>, error>` |
| `stats` | `(id) -> result<stream<sample>, error>` |
| `exec` | `(id, command, tty, cols, rows) -> result<string, error>` |
| `exec-attach` / `exec-write` / `exec-resize` / `exec-close` | the owned exec session |
| `list-images` / `pull` / `remove-image` | images; `pull` is a stream |
| `list-volumes` / `create-volume` / `remove-volume` | named volumes |
| `list-networks` | `() -> list<network>` |

`create-request` refuses bind mounts, privileged mode, host namespaces and
devices. No function returns an environment value.

## `anthracite:store/catalog@1.0.0`

`apps/store/wit/store.wit`

| Function | Shape |
| --- | --- |
| `sources` | `() -> list<source>` |
| `refresh` | `(source) -> result<source, error>` |
| `search` | `(query, categories, limit) -> list<app-summary>` |
| `app` | `(id) -> option<app-detail>` |
| `versions` | `(id) -> list<version>` |
| `download` | `(id, version) -> result<stream<list<u8>>, error>` |
| `check-updates` | `(installed) -> list<update>` |

`download` yields only bytes that matched the index's size and SHA-256 before
the first chunk. There is no install or approve function in this package; the
manager uses the engine API with the operator session.
