# Report — Anthracite v0.7.0 / M6

## Summary

`v0.7.0` ships the operated edge. Nine phases from [§29](../../design/29-v0.7-plan.md)
ran as one PR each into `v0.7.0`, and the final tag `v0.7.0` is cut at
`1f09a3a`. The milestone adds five business apps (`certs`, `acme`, `proxy`,
`containers`, `store`) on top of four platform mechanisms: manifest v4 with
declared roots/listeners/egress, sandbox L1 with per-service uids and Landlock,
the privileged-port capability, and the service broker/audit/surface exports.

The edge composition is `compositions/edge.toml` (seven system apps plus the
five). The removal matrix is a signed golden test, and a removal can be an
operator action through `[composition] allow-degraded`: the consumer degrades
with `unresolved` instead of the whole document failing to boot.

## Integrated phases

| Phase | PR | State | Merge |
| --- | --- | --- | --- |
| 1 — Manifest v4 and the accepted decisions | [#194](https://github.com/janitochvvll2311/anthracite-panel/pull/194) | merged | squash |
| 2 — Sandbox L1 and the port capability | [#195](https://github.com/janitochvvll2311/anthracite-panel/pull/195) | merged | squash |
| 3 — Broker, audit bridge and surface exports | [#196](https://github.com/janitochvvll2311/anthracite-panel/pull/196) | merged | squash |
| 4 — Certificate manager | [#197](https://github.com/janitochvvll2311/anthracite-panel/pull/197) | merged | squash |
| 5 — Reverse proxy | [#199](https://github.com/janitochvvll2311/anthracite-panel/pull/199) | merged | squash |
| 6 — ACME | [#200](https://github.com/janitochvvll2311/anthracite-panel/pull/200) | merged | squash |
| 7 — Containers | [#201](https://github.com/janitochvvll2311/anthracite-panel/pull/201) | merged | squash |
| 8 — Store and the manager handoff | [#202](https://github.com/janitochvvll2311/anthracite-panel/pull/202) | merged | squash |
| 9 — Hardening and release | [#203](https://github.com/janitochvvll2311/anthracite-panel/pull/203) | merged | squash |

## Delivered

### Platform

- **Manifest v4**: optional `roots`, `listen`, `egress` and `{broker}`; grammar
  validated, shown at approval and frozen in the grant/huella. v3 remains
  accepted. `ServiceView` marks roots/ports `enforced` and egress `intent`.
- **Sandbox L1**: per-service uid when the engine is root, Landlock over the
  declared and implicit paths, file-root rights for sockets, missing roots
  denied, `CAP_NET_BIND_SERVICE` for declared privileged listeners. A refused
  launch is audited as `service.deny` and not retried.
- **Broker and audit**: private `{broker}` sockets, generated clients, grant
  checks, `anth:audit/record@1.0.0` with sanitized detail, and surface
  `exports` so the manager finds providers by interface.
- **`[composition] allow-degraded`**: off by default. When on, an import with no
  provider marks the consumer `unresolved` (with `detail.interface`) and the rest
  of the composition boots. Wired through daemon, panel host, boot, API and the
  broker view.

### Apps

- **`certs`** — inventory, import/validate, atomic publish to
  `{shared}/tls/<id>` at `0600`, `auto-renew` for ACME material; private keys
  never leave the service through WIT, HTTP or the panel.
- **`acme`** — RFC 8555 accounts and orders, HTTP-01 through the proxy,
  DNS-01 with `manual`, `rfc2136` and `webhook`, renewal by `auto-renew`/ARI,
  revocation, CA allowlist and staging separation.
- **`proxy`** — HTTP/HTTPS vhosts, SNI/TLS reload, path routes, WebSocket,
  HTTP/2, redirect/HSTS, header policy, whole-document `apply` with rollback,
  lossy access log with a dropped counter, panel modals/FAB.
- **`containers`** — Docker/Podman inventory, lifecycle, logs, stats, images,
  volumes, networks and exec; no bind mounts, privileged mode or host
  namespaces; mutations audited without environment values.
- **`store`** — signed indexes over exact bytes, `native`/`github` sources,
  cache, search, digest-pinned download and the manager handoff. The store
  never installs; the manager holds the operator session and shows the
  engine-computed risk.

### Release and docs

- `scripts/release.sh` signs the five new bundles and builds the keyed binaries;
  `release-bundles.sh` requires the eight emulator demos and builds
  `SHA256SUMS` and the archive from one list.
- `docs/guide/` is the 0.7 manual: pages for the five apps, the WIT interface
  reference, ports/roots/capabilities and approval text, the manual QA matrix,
  and the release notes.
- `compositions/edge.toml`, `tests/fixtures/expected/edge.json` and the golden
  removal test are the architecture of record for the edge.

## Review findings addressed

- **Proxy**: read-only mode, modal/FAB actions, listener bind selection and
  access-log budget.
- **ACME**: renewal bound, token modes, fchmod, owner/group checks and the
  HTTP-01 self-check.
- **Containers** (five rounds): socket approval/audit/traversal, missing-root
  audit, `setgroups(0)`, symlink canonicalization and tests.
- **Store** (three rounds): `Limited` bodies, resolver-level SSRF guard, unique
  cache temporaries, store-scoped replay `sequence`, channel/semver updates,
  global search ordering, signer fingerprint verification and deterministic e2e.
- **Hardening**: edge counts, removal expectations, Pebble command, the
  `unresolved` contract, release-bundles demo list and QA evidence.

## Tests

- `cd engine && cargo test --workspace`: all suites pass, including 25 golden
  tests (edge surface, five individual removals, all five) and the boot
  allow-degraded/strict cases.
- `cd sdk/rust && cargo test --workspace` and
  `cd panel/backend && cargo test --workspace`: pass.
- App suites: certs 12, ACME RFC 8555 fixture, proxy 24+12, containers Engine
  API fixture, store 7 unit + loopback (tamper, digest, replay, repoint).
- Browser e2e against the edge panel: **20 passed, 2 skipped** (store handoff
  skipped because `hello` was already installed).
- `scripts/acid-test.sh`: an external bundle installs, composes, serves and
  answers.
- `cargo fmt --all --check` and `clippy -D warnings` in engine, sdk/rust and
  panel/backend.
- A throwaway-key release of the five bundles passed conformance, and
  `release-bundles.sh` produced 21 bundles with `sha256sum -c` OK; a missing
  demo exits 1.

## Decisions

- [ADR 0001](../../decisions/0001-can-the-proxy-document-open-listeners.md) — the
  proxy document selects only the argv binds.
- [ADR 0002](../../decisions/0002-how-does-the-document-express-https-redirect-hsts-forwarded-head.md)
  — redirect, HSTS, forwarded headers, body limits and dropped log events.
- [ADR 0003](../../decisions/0003-how-are-the-tls-and-acme-directories-under-shared-passed-when-ar.md)
  — one `{shared}` and derived TLS/ACME directories.
- [ADR 0004](../../decisions/0004-how-does-the-acme-client-allowlist-ca-directories-and-opt-into-a.md)
  — CA allowlist and `allow-private` opt-in.
- [ADR 0005](../../decisions/0005-how-does-a-manual-dns-01-order-get-its-confirmation.md)
  — manual DNS-01 confirmation.
- [ADR 0006](../../decisions/0006-where-does-an-issued-certificate-key-live-when-the-certificate-s.md)
  — issued key custody and `issued-unstored`.
- [ADR 0007](../../decisions/0007-how-does-the-panel-know-which-ca-directories-are-staging.md)
  — staging directories in the capability record.
- [ADR 0008](../../decisions/0008-how-is-the-acme-gate-automated-when-ci-cannot-download-or-run-pe.md)
  — RFC 8555 fixture as the gate, Pebble as QA.
- [ADR 0009](../../decisions/0009-how-does-the-container-manager-hold-the-runtime-socket.md)
  — runtime socket as a literal argv root.
- [ADR 0010](../../decisions/0010-how-do-exec-sessions-work-and-how-is-the-runtime-tested-without-.md)
  — exec sessions and the Engine API fixture.
- [ADR 0011](../../decisions/0011-where-does-the-store-s-catalogue-trust-live-and-who-installs.md)
  — catalogue trust root and manager-only installs.
- [ADR 0012](../../decisions/0012-when-is-a-bundle-digest-checked-relative-to-the-download-stream.md)
  — digest checked before the first streamed chunk.

D9–D20 in [§16](../../design/16-decision-log.md) remain binding.

## Out of scope

- Egress enforcement (L2), seccomp (L3 second half) and cgroups (L4).
- TLS-ALPN-01, wildcard issuance without DNS-01, vendor DNS adapters.
- Kubernetes, Swarm, Compose stacks, image builds, bind mounts or privileged
  containers.
- Store payments, licensing, reviews, auto-update or a hosted catalogue.
- Windows/macOS host services; `anth login/logout` over HTTP; Pulley.

## Pending and risks

- **Real release key**: the ceremony is implemented and was exercised with a
  throwaway key; the operator must hold the private half and run
  `scripts/release.sh` for the published artifacts and images.
- **CI runner**: GitHub Actions still schedules no runner, so every job fails in
  about two seconds; the local suite is the gate until that changes.
- **Deployment host**: Let's Encrypt staging/production, Docker and rootless
  Podman have fixture coverage and manual QA rows but have not been exercised
  on a real host in this environment.
- **Egress is intent**: a compromised service is bounded by roots and ports,
  not by a network policy until v0.8.
- **Container runtime socket**: root-equivalent for containers; the approval and
  docs say so.
