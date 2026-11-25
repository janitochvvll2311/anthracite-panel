# Store

A signed catalogue: browse apps, refresh sources, search, compare versions and
download a digest-pinned bundle. The service never installs or approves; the
panel's Manager performs the install with the operator session and the engine
computes the risk.

Bundle `org.anthracite.store`, package `anthracite:store@1.0.0`, class `socket`,
one `panel.view`, one FAB and one modal.

## Authority

```toml
[service]
class = "socket"
argv = ["--socket", "{socket}", "--data", "{data}", "--shared", "{shared}",
        "--broker", "{broker}"]
egress = ["any"]
```

- Sources are configured by the operator in `{data}/sources.json` (0600). Each
  entry names a `kind` (`native`, `github` or `local`), a URL and an Ed25519
  **public key**: that key is the catalogue's trust root, and an index cannot
  introduce its own.
- A private or loopback URL needs `allow-private`; a missing key, a bad signature
  or a schema other than 1 is refused.
- The service has no operator session and no install verb. It caches indexes and
  bundles under `{data}/cache` and serves offline from them.
- `local` sources live under `{shared}/store/<directory>` and need no network.

```json
{
  "sources": [
    {
      "id": "example",
      "name": "Example Store",
      "kind": "native",
      "url": "https://store.example",
      "public-key": "…64 hex…",
      "allow-private": false
    },
    {
      "id": "airgap",
      "name": "Air-gapped",
      "kind": "local",
      "url": "airgap",
      "public-key": "…64 hex…"
    }
  ]
}
```

A `github` source's URL is the release API endpoint; the release must carry
`index.json` and `index.json.sig` assets.

## Index and trust

`index.json` is verified over the **bytes as served**, with a detached
`index.json.sig` (standard base64 Ed25519). The publisher signs the catalogue
with the same tool that signs bundles:

```sh
anth-sign keygen --out store.key
anth-sign file index.json --key store.key   # writes index.json.sig
```

The index carries a monotonic `sequence` (per store). A refresh whose signature is
valid but whose sequence is older than the cached one is refused as a replay, so
a mirror cannot resurrect yanked or vulnerable versions. The index is advisory:
the store shows publishers, digests, yanked versions and
signer fingerprints, but the engine verifies the bundle signature, manifest,
WIT and grant. The store's "trusted" flag means the source's key verifies the
cached index, not that the publisher is trusted by the keyring.

Indexes are capped at 8 MiB; bundles at 32 MiB (the HTTP body is bounded while
reading, not after buffering). The public resolver refuses private, loopback,
link-local, CGNAT and IPv4-mapped addresses at connection time, so a DNS rebind
cannot bypass the pre-flight check. Updates follow the installed channel
(`stable` by default) and compare semantic versions; a downgrade is not offered. A refresh that fails signature
verification or parsing leaves the previous cache untouched. A cached index or
bundle is re-verified on every read; a corrupt cache is evicted, not served.

## Surface

| Route | Function |
| --- | --- |
| `GET /sources` | configured sources and their trust/age |
| `POST /sources/refresh` | fetch, verify and cache one index |
| `GET /search?query=…&limit=…` | summaries across cached sources |
| `GET /app?id=…` | details and versions |
| `GET /versions?id=…` | versions only |
| `GET /download?id=…&version=…` | verified bundle stream (`application/octet-stream`) |
| `POST /updates` | compare an installed list; reports, never applies |

CLI (read-only): `anth app store sources|search|show|versions|updates`.

The view has a search box, cards and a details pane with the versions, digest,
size, channel and yanked state. The FAB refreshes the catalogues (or opens the
Sources modal, which lists the configured sources and refreshes one). The view
says explicitly that installing happens in **Manager → Store**.

## Manager handoff

The manager discovers a composed app exporting
`anthracite:store/catalog@1.0.0` through the engine surface, reads the search and
details with the operator session, downloads the bundle, checks its SHA-256 and
size, and POSTs it to `/api/engine/apps`. The engine computes the grant; the
manager shows the risk, service class/argv, interfaces and authority diff and
requires explicit approval. If the bundle signer is not in the keyring and not
the distribution key, the handoff shows the fingerprint, lets the operator add
the public key (explicitly, out of band) and only then approve. The store never
sees the session or the grant.

## Tests

- Unit: index parsing/schema/digests, search/updates, source validation, bundle
  path safety, signature verification, base64 key parsing.
- Loopback: a signed native index, a GitHub-shaped release API and a local
  source; refresh, trust, search, details, versions, updates and download; a
  tampered signature keeps the previous cache; a digest mismatch is refused
  before the stream.
- Browser e2e: the manager Store tab installs the fixture bundle through the
  engine and approves it; the store app view lists the catalogue. Both skip
  unless the composition carries the Store app and the fixture source exists.
- The fixture helper is `scripts/store-fixture.sh`, which builds a signed
  `apps/hello` bundle and a signed index under the configured shared directory.

## Decisions

- 0011 — the source public key is the catalogue trust root; verification is over
  the exact bytes; the manager owns install/approval.
- 0012 — a bundle is verified (size and SHA-256) before the stream starts; a bad
  digest is an error, never a partial download.

## Limits

OCI sources, an in-app install button, auto-update, ratings, payments and a
hosted store are out of scope. Icons come from the installed bundle, not remote
image URLs.
