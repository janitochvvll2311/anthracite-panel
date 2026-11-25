# Store

The store app browses signed catalogues. It searches, caches indexes and bundles,
verifies every read and streams a digest-pinned bundle. It has no install verb:
the panel manager performs the install with the operator's session.

Design: [§27](../design/27-app-store.md). Decisions: [ADR 0011](../decisions/0011-where-does-the-store-s-catalogue-trust-live-and-who-installs.md),
[0012](../decisions/0012-when-is-a-bundle-digest-checked-relative-to-the-download-stream.md).
The catalogue format is also documented in [`apps/store/README.md`](../../apps/store/README.md).

## Approval

| Field | Value |
| --- | --- |
| App id | `org.anthracite.store` |
| Service class | `socket` |
| Egress | `any` (sources are operator-configured) |
| Interface | `anthracite:store/catalog@1.0.0` |

The store has no operator session, no engine API credential and no install verb.
A source is trusted by its own Ed25519 public key in `{data}/sources.json`; the
index signature is verified over the exact bytes served. The bundle's signer is
a **second, independent** key, checked by the engine when the manager installs.
The UI keeps the two fingerprints separate.

## Index

```json
{
  "schema": 1,
  "sequence": 1,
  "store": {"id": "org.example.store", "name": "Example"},
  "apps": [
    {
      "id": "org.example.notes",
      "name": "Notes",
      "summary": "A tiny notes app.",
      "publisher": "Example",
      "categories": ["productivity"],
      "versions": [
        {
          "version": "1.2.0",
          "channel": "stable",
          "published_at": 1700000000,
          "sha256": "...",
          "size": 1234,
          "bundle_url": "notes.anth",
          "signer": "...",
          "yanked": false
        }
      ]
    }
  ]
}
```

`sequence` is required, positive and monotonic per `store.id`; a valid but older
catalogue is refused as a replay and the cache is kept. `latest` prefers
`stable`; `check-updates` follows the installed channel and never offers a
downgrade. Indexes are capped at 8 MiB and bundles at 32 MiB while reading. The
public resolver refuses private, loopback, link-local, CGNAT and IPv4-mapped
addresses at connection time, so a DNS rebind cannot bypass the pre-flight check.

## Manager handoff

1. The manager finds the composed app exporting `anthracite:store/catalog@1.0.0`.
2. It reads the catalogue and the selected version through the app route.
3. It downloads the stream, checks size and SHA-256, then `POST`s the bundle to
   `/api/engine/apps`.
4. The engine verifies signature, manifest and WIT; the manager shows the
   computed risk, service class/argv and authority diff and requires explicit
   approval. An untrusted signer adds an explicit `key add` (the pasted key is
   checked against the catalogue's fingerprint first).
5. The store never sees the session or the grant; the engine audit is the record.

## Routes and CLI

| Route | CLI | Function |
| --- | --- | --- |
| `GET /sources` | `anth sources` | configured sources |
| `POST /sources/refresh` | — | refresh one index |
| `GET /search` | `anth search` | search |
| `GET /app` | `anth show` | details |
| `GET /versions` | `anth versions` | versions |
| `GET /download` | — | verified bundle stream |
| `POST /updates` | `anth updates` | updates for installed apps |

## Tests

`apps/store/backend/tests/loopback.rs` signs a fixture index and bundle, then
covers tamper, digest mismatch, replay, repointing a source at another store and
the download stream. `tests/e2e/src/specs/store.spec.ts` covers the view and the
manager handoff, including the signer prompt. `scripts/store-fixture.sh` builds
the fixture a developer runs locally.
