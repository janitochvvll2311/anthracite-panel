# Release notes

## 0.7.0 — the edge composition

`v0.7.0` is the M6 milestone: five business apps, the sandbox L1 and the port
capability, proven on the edge composition of
[§21 §3](../design/21-next-version-apps.md#3-the-edge-composition).

### Five apps

- **`certs`** — inventory, import, validate and publish certificates; the proxy
  reads `{shared}/tls/<id>/`. Private keys are never returned by a route or WIT.
- **`acme`** — RFC 8555 issuance and renewal; HTTP-01 through the proxy,
  DNS-01 through `manual`, `rfc2136` or a `webhook`; staging is never promoted.
- **`proxy`** — HTTP/HTTPS vhosts, WebSocket, HTTP/2, whole-document apply with
  rollback, TLS reload and a lossy access log that reports its drops.
- **`containers`** — Docker-compatible lifecycle, logs, stats, images, volumes
  and exec behind the approved runtime socket; no bind mounts or privileged mode.
- **`store`** — signed catalogues, digest-pinned downloads and the manager
  handoff. The store never installs; the manager holds the operator session.

### Platform

- **Manifest v4**: `roots`, `listen`, `egress` and `{broker}`; the intent is
  shown at approval and frozen. v3 manifests are still accepted.
- **Sandbox L1**: per-service uid when the engine is root, Landlock roots,
  `CAP_NET_BIND_SERVICE` for declared privileged listeners, and the existing L0
  process hygiene. A kernel that cannot apply L1 refuses the launch.
- **Broker, audit bridge and surface exports**: services call each other through
  `{broker}`; `anth:audit/record@1.0.0` writes sanitized records; the composition
  surface lists each app's exports, so the manager finds providers by interface.
- **`[composition] allow-degraded`** (default `false`): an import with no
  provider is fatal unless the operator opts in; then the consumer degrades with
  `unresolved` and the rest boots. This is what makes removing one of the five
  apps a supported operation.
- A service that declares an egress list has it shown as **intent**; network
  isolation (L2) is not in this release.

### Removal

Removing an app leaves a working system with that section gone. The golden
matrix in `engine/api/tests/golden.rs` asserts zero fatal and the exact degraded
set for each removal. Uninstalling never deletes `{data}` or files published
under `{shared}`; cleanup is explicit.

### Upgrade

Back up `$state`, verify the release artifacts, stop the process, replace the
binaries and bundles, start again, then check `anth status` and `anth audit
--limit 20`. State from 0.6.x is read by 0.7.0; a downgrade is not supported
after a 0.7.0 binary has written the state. A release build still refuses a
system bundle signed by another key, so a key rotation is a release.

### Known limitations

- Egress is intent until L2. A compromised service is stopped by L1 roots and
  ports, not by a network policy.
- The container runtime socket is root-equivalent for containers; the approval
  says so.
- ACME production CAs, Docker and rootless Podman are manual QA rows; the
  reproducible half is the RFC 8555 fixture and the Engine API fixture.
- The release is only as strong as the key ceremony: the private key stays with
  the operator, and `scripts/release.sh` compiles only the public half in.
- There is no debug-preview release (D19).
