# Edge composition: manual QA

The automated half of the release gate is the local suite: four Rust
workspaces, the browser e2e against a running panel, the golden removal matrix
and the edge composition. The rows below are what a human runs on a deployment
host before `v0.7.0` is published. They are not claims: a row that was not run
is recorded as not run.

## Before starting

1. Build the release with a real key: `scripts/release.sh --key <private-key>`.
   It produces the keyed binaries and the signed bundles, including `certs`,
   `acme`, `proxy`, `containers` and `store`, and verifies each bundle's tier.
2. Install the 12 bundles of `compositions/edge.toml`, approve each with its
   declared class, and copy the composition to `$state/composition.toml`.
3. Keep `[composition] allow-degraded = false` for the normal run; turn it on
   for the removal rows, as the table says.

## Matrix

| # | Row | Steps | Expected |
| --- | --- | --- | --- |
| 1 | Certificate import and HTTPS | import a chain+key in `certs`, publish it, add a proxy host that uses it, `curl -k https://<host>` | the proxy answers with the imported certificate; the key never appears in the UI or a log |
| 2 | Let's Encrypt staging, HTTP-01 | add the staging directory to the ACME allowlist, issue for a test name, let the proxy serve the challenge | the order completes, `certs` stores it, the proxy serves it; the order is marked staging |
| 3 | Renewal without an operator | set `auto-renew` on the staging certificate, shorten its lifetime, wait for the worker | the order renews at about two thirds of lifetime; the timeline shows it; no operator action |
| 4 | DNS-01 manual | issue with `manual`, copy the TXT record, confirm | the client waits for the confirmation, verifies the record and completes |
| 5 | DNS-01 webhook | configure a webhook solver against a test endpoint | the endpoint presents and cleans the record; no secret is logged |
| 6 | TLS reload | replace the published certificate and `POST /certificates/reload` | new connections use the new certificate; in-flight requests are not dropped; `status` reports the reload |
| 7 | Two hosts, WebSocket, HTTP/2 | route host A to a WebSocket echo and host B to an upstream; make an `h2` request | both hosts answer, the echo upgrades, ALPN negotiates `h2` |
| 8 | Docker | point `containers` at `/var/run/docker.sock` on a Docker host | list, start, stop, logs and exec work; a bind mount or `--privileged` is refused; no environment value in a log |
| 9 | Rootless Podman | point it at the user's `podman.sock` | the same rows work; the approval names that socket |
| 10 | Store: local source | configure a `native` source over a directory or loopback HTTPS, search, install | the manager shows the digest, risk and signer; approval installs the bundle |
| 11 | Store: remote source | configure a `github`-shaped or HTTPS source | the index verifies, a bundle downloads, the same handoff runs |
| 12 | Tampered bundle | flip a byte in the cached bundle and download again | the store refuses before the first chunk and evicts the cache entry; the manager never sees a bundle |
| 13 | Removal matrix | remove `certs`, `proxy`, `acme`, `containers` and `store` one at a time, then all five; set `allow-degraded = true` when a consumer stays composed | `certs`: the proxy keeps published files and `acme` degrades with `unresolved` (it does not run); `proxy`: HTTP-01 fails its self-check, DNS-01 still works; `acme`: no renewals; `containers` and `store`: no other degradation; all five leaves the seven system apps of `edge.toml` running |
| 14 | No secrets anywhere | grep the routes, logs, audit records, crash dumps and the panel HTML for PEM blocks, tokens, `Authorization`, container env values | nothing found |

## Automated evidence

Run these first; they cover the reproducible half.

```sh
(cd engine && cargo test --workspace)
(cd sdk/rust && cargo test --workspace)
(cd panel/backend && cargo test --workspace)
(cd apps/certs/backend && cargo test)          # PEM, key match, expiry
(cd apps/acme/backend && cargo test)           # RFC 8555 fixture
(cd apps/proxy/backend && cargo test)          # routes, apply/rollback
(cd apps/containers/backend && cargo test)     # Engine API fixture
(cd apps/store/backend && cargo test)          # unit + loopback
(cd engine && cargo test -p anth-api --test golden)  # removal matrix

scripts/acid-test.sh                            # external bundle
scripts/acme-pebble.sh --run                    # terminal 1
(cd apps/acme/backend && \
    ANTH_PEBBLE_DIRECTORY=https://localhost:14000/dir \
    ANTH_PEBBLE_CA=/tmp/anthracite-panel/pebble/test/certs/pebble.minica.pem \
    cargo test --test pebble -- --ignored)

(cd tests/e2e && ANTH_PANEL_URL=... ANTH_PANEL_PASSWORD=... \
    ANTH_CONTAINERS_FIXTURE=1 ANTH_STORE_FIXTURE=1 \
    ANTH_STORE_PUBLIC_KEY=... ANTH_STORE_SIGNER=... bun run test)
```

The host-specific rows (1–12) have fixture coverage in `apps/*/backend/tests`
and `tests/e2e`; a fixture substitutes for the real CA, Docker or Podman only in
the automated half, never in this matrix.
