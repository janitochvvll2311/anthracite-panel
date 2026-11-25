# 27. App store

> Part of the [design index](../../DESIGN.md). Scope for **v0.7.0 / M6**.
> Start conditions, prerequisites and sequencing: [§21](21-next-version-apps.md).

Bundle `org.anthracite.store`, package `anthracite:store@1.0.0`, service class
`socket`, one `panel.view`.

## 1. Scope

- Aggregate one or more **sources**: a signed static index, an OCI artifact
  repository, a GitHub releases index, or a local directory for air-gapped
  installs.
- Search and browse by name, publisher, category, interface and tier hint.
- App details: description, license, homepage, changelog, versions, channels,
  declared interfaces, service class and argv (from the signed manifest after
  download), bundle digest and signer fingerprint.
- Download a version as a digest-pinned stream into the cache.
- Compare installed versions with the catalogue and report updates. **No silent
  update, no auto-install, no auto-approval.**
- Cache indexes and bundles, verify every read, and serve offline from the cache.
- Show yanked versions and index revocations as warnings; the engine's trust is
  still authoritative.

**Non-goals:** payments, licensing enforcement, ratings/reviews, telemetry,
auto-update, a hosted store, or an app-visible operator session.

## 2. The authority boundary

The store is a **catalogue**, not an installer.

- The store service has `socket` egress to configured index and bundle URLs, and
  a `{data}` cache. It has no operator session, no engine API credential and no
  install verb.
- The panel's manager performs the install with the operator's session:
  1. The manager finds composed apps whose surface exports
     `anthracite:store/catalog@1.0.0` ([§22 §6](22-platform-prerequisites.md#6-surface-exports)).
  2. It reads the catalogue and the selected version through that app's route,
     with the same session and origin checks as any app call.
  3. It downloads the bundle as a stream, checks the advertised SHA-256 and the
     size cap, then `POST`s it to `/api/engine/apps`.
  4. The engine verifies the signature, the manifest and the WIT; it computes
     the grant. The manager shows the computed risk, the service class/argv, the
     declared interfaces and the authority diff, and requires explicit approval.
     When the bundle's signer is not in the keyring and is not the distribution
     key, the handoff shows the fingerprint and offers an explicit `key add`
     before the install; the store cannot vouch for a publisher by itself.
  5. The store is told nothing about the session or the grant; the engine's
     audit is the record.

This keeps D-Manager-Panel's rule: an app module never holds engine management
authority. The store view browses; the service caches; the install button lives
in the manager, where the session already is.

**Proposed alternative (not v0.7.0):** a generic `panel.intent` vocabulary,
where an app asks the panel to perform an install with a digest and the panel
confirms. It is a contract addition and is deferred until the manager flow
proves too clumsy. The default is the manager-mediated flow above.

## 3. WIT outline

```wit
package anthracite:store@1.0.0;

interface catalog {
  use anth:error/kinds@1.0.0.{error};

  enum channel { stable, beta, nightly }

  record source { id: string, name: string, kind: string, url: string, trusted: bool }
  record app-summary { id: string, name: string, summary: string, publisher: string, categories: list<string>, latest: string }
  record version {
    version: string,
    channel: channel,
    published-at: u64,
    sha256: string,
    size: u64,
    bundle-url: string,
    signer: string,
    yanked: bool,
  }
  record app-detail {
    id: string,
    name: string,
    summary: string,
    description: string,
    publisher: string,
    license: string,
    homepage: option<string>,
    categories: list<string>,
    versions: list<version>,
  }
  record installed { id: string, version: string, channel: channel }
  record update { id: string, installed: string, latest: string, channel: channel }

  sources: func() -> list<source>;
  refresh: func(source: string) -> result<source, error>;
  search: func(query: string, categories: option<list<string>>, limit: u32) -> list<app-summary>;
  app: func(id: string) -> option<app-detail>;
  versions: func(id: string) -> list<version>;
  download: func(id: string, version: string) -> result<stream<list<u8>>, error>;
  check-updates: func(installed: list<installed>) -> list<update>;
}
```

`download` is a stream route. The service verifies the cached or fetched bytes
against `sha256` before the first chunk; a mismatch is an error before the
stream starts, not a truncated bundle.

## 4. Routes and CLI

| Surface | Shape |
| --- | --- |
| `GET /sources` | `sources` |
| `POST /sources/refresh` | `refresh` (body carries the source id) |
| `GET /search?query=…&limit=…` | `search` |
| `GET /app?id=…` | `app` |
| `GET /versions?id=…` | `versions` |
| `GET /download?id=…&version=…` | `download`, `stream = true`, `application/octet-stream` |
| `POST /updates` | `check-updates` (body carries the installed list) |
| CLI `store sources`, `search`, `show`, `updates` | the same calls |

The CLI is read-only for installs by design; there is no `store install` verb.

## 5. Index format and trust

A source serves two files:

```
index.json        the catalogue, exact bytes
index.json.sig    detached Ed25519 signature over those exact bytes
```

- The signature is over the **bytes as served**, not a reserialized JSON. This
  avoids a canonical-JSON argument and matches the bundle rule: verify what you
  received.
- The public key is configured in the store service and shown at approval. An
  index cannot introduce its own trust root.
- A publisher signs the exact bytes with `anth-sign file index.json --key
  <private-key>`, which writes `index.json.sig`. The same tool that signs bundles
  signs the catalogue, so there is one signature format and one key-format rule
  ([§9.1](09-authority.md#91-tiers-and-signing)).
- The index is advisory metadata. The bundle's signature, manifest hash, WIT and
  grant are verified by the engine; the store's "publisher" label is not a trust
  decision.
- The index may list yanked versions and revoked signers. The UI warns; the
  engine refuses what its keyring or grant refuses.

Minimal index:

```json
{
  "schema": 1,
  "store": { "id": "org.example.store", "name": "Example Store" },
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
          "published_at": 1760000000,
          "sha256": "…",
          "size": 48213,
          "bundle_url": "https://store.example/notes-1.2.0.anth",
          "signer": "…",
          "yanked": false
        }
      ]
    }
  ]
}
```

## 6. Provider-agnostic sources

- `native`: the signed index above over HTTPS.
- `oci`: an OCI repository whose artifact manifest points at the index and
  bundle blobs; anonymous pull in v1.
- `github`: release assets, with the index and signature as assets.
- `local`: a directory under `{shared}/store` for offline/air-gapped installs;
  the root is `--local-root {shared}/store` in argv.

Vendor-specific stores are sources, not separate provider bundles. The
composition binds one provider per interface id, and
`anthracite:store/catalog@1.0.0` is documented as a first-party interface. The
contract classifies the `anthracite` namespace as first-party but does **not**
yet enforce tier ownership, so a third-party provider is not a supported
extension point; the official store aggregates sources. A future
optional-interface negotiation could change that, but it is not v0.7.0
([§21 §8](21-next-version-apps.md#8-decisions), P-Store-Provider).

## 7. UI

The view is a search box, category filters and a card grid. A details page shows
versions, changelog, license, declared interfaces and the source's trust state.
The view is browse-only: the install button lives in the manager's Store tab,
which reads the same provider and owns the session. The FAB "Refresh catalogues"
is icon-only; a second action opens the sources modal.

Icons come from the bundle after download, not from remote image URLs, so the
panel does not leak the operator's address to a catalogue. Before download, the
card uses initials.

## 8. Dependencies and phases

| Phase | Delivers | Depends on |
| --- | --- | --- |
| S1 | Index format, signature verification, native source, cache, search | [§22 §2](22-platform-prerequisites.md#2-listener-and-egress-intent-in-the-manifest), [§22 §3](22-platform-prerequisites.md#3-secret-custody), [§22 §4](22-platform-prerequisites.md#4-streams-quotas-and-backpressure) |
| S2 | View, details, source management, refresh | S1 |
| S3 | Manager store tab, download handoff, install/approval flow | S2, [§22 §6](22-platform-prerequisites.md#6-surface-exports) |
| S4 | Updates, channels, yanked/revocation warnings | S3 |
| S5 | OCI, GitHub and local sources, offline mode, hardening | S4, [§22 §5](22-platform-prerequisites.md#5-system-tier-and-service-sandbox) |

## 9. Risks

- **Malicious catalogue.** The index is signed, bundles are digest-pinned and the
  engine verifies signatures. The store never approves.
- **SSRF and privacy.** Source URLs are operator-configured. The service refuses
  loopback, link-local and metadata addresses unless explicitly allowed, and
  remote icons are not fetched by the panel.
- **Cache poisoning.** Cache keys include the digest; a read re-hashes. A
  corrupted cache is evicted, not served.
- **Namespace squatting.** The engine's id and signer rules decide; the store
  displays, it does not arbitrate.
- **Stale metadata.** ETag/If-None-Match and a visible `refreshed_at`; offline
  mode labels the age.

## 10. Acceptance

- A signed index with one app searches, shows details and downloads a bundle
  whose digest matches.
- A tampered index signature is refused and the previous cache is kept.
- A tampered bundle is refused before the manager can install it.
- The manager installs the bundle, shows the engine-computed risk and requires
  approval; the store never sees the session.
- An update is reported, not applied; applying it re-runs install and approval.
- A local source works with no network.

---

## 11. Implementation notes

The source public key in `{data}/sources.json` is the catalogue trust root
(decision 0011); the index signature is verified over the exact bytes served and
a failed refresh keeps the previous cache. A bundle is fully fetched, checked
against its size and SHA-256 and cached before the first streamed chunk, so a
mismatch never reaches the manager as a partial file (decision 0012). The store
has no install verb: the manager discovers the provider through the composition
surface, downloads with the operator session, POSTs the bundle to the engine and
requires the engine-computed risk and explicit approval. Sources may be
`native`, `github` or `local`; the always-on test uses a signed native fixture,
a GitHub-shaped release API and a local directory.
