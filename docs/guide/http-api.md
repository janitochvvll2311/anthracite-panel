# HTTP API

The engine's API is a set of **resources**. It is served over the Unix socket
(`/api/...`) and, when `listen` is configured, over TCP. The official panel
mounts the same router in-process; a panel in another process uses TCP with a
session.

```sh
# Unix socket (the CLI does this for you)
curl --unix-socket /run/anth/engine.sock http://localhost/api/health

# TCP with a session
curl -c cookies -H 'content-type: application/json' \
  -d '{"operator":"operator","password":"…"}' http://127.0.0.1:8080/api/engine/session
curl -b cookies http://127.0.0.1:8080/api/engine/apps
```

## Authentication

**Unix socket.** The kernel supplies the peer uid. The engine accepts its own
uid and root; there is no token and no login. `anth` uses this path.

**TCP.** A session is a bearer token in an `HttpOnly; SameSite=Strict` cookie
named `anth_session`, minted by:

```http
POST /api/engine/session
{"operator": "operator", "password": "…"}
```

`GET` returns the current session, `DELETE` logs out. The session lives for
`[auth] session-ttl` (12 hours by default). A password change revokes every
session, and a TCP client gets a fresh cookie in the same response.

**First boot.** Until the generated password is changed, only the operator
resource and the session routes are reachable; everything else answers
`403 password-change-required`. The panel's `/login` and `/setup` pages walk
through it. `GET /api/engine/operator` reports the identity;
`PATCH /api/engine/operator` changes the password (a session must send
`current_password`; the socket's peer credential does not).

**Origin and headers.** Unsafe methods require `Origin` to match `Host` (or
`X-Forwarded-Host` from a loopback peer). A request without `Origin` — the CLI,
`curl` — authenticates normally. Every response carries a restrictive CSP
(`default-src 'none'; frame-ancestors 'none'; base-uri 'none'`), `nosniff`,
`no-referrer` and `X-Frame-Options: DENY`. Login failures are rate-limited per
source: five in a minute block the source for 60 seconds.

## Errors

Every failure is a JSON envelope; the HTTP status follows the kind:

```json
{"error": {"code": "denied", "message": "…", "app": "org.example.notes", "detail": {}}}
```

| HTTP | `code` | Meaning |
| --- | --- | --- |
| 400 | `invalid` | the request or a document is wrong |
| 401 | `unauthenticated` | no session, or the password is wrong |
| 403 | `denied` | the authority or the signature refused it |
| 403 | `password-change-required` | the first-boot gate |
| 404 | `not-found` | no such app, key or route |
| 405 | `method-not-allowed` | the route exists with another method |
| 409 | `conflict` | the id exists, or dependents need an acknowledgment |
| 412 | `precondition-failed` | `If-Match` did not match |
| 413 | `too-large` | the body exceeds the cap |
| 429 | `rate-limited` | too many login failures |
| 500 | `internal` | a bug or an unreadable state |
| 503 | `unavailable` / `exhausted` | not running, degraded, or out of budget |

`detail` carries what the client needs: the dependents of a breaking action and
the acknowledgment token, the required approval levels, the caller's error.

## Resources

| Resource | GET | PUT | PATCH | DELETE | POST |
| --- | --- | --- | --- | --- | --- |
| `/health` | liveness, unauthenticated | — | — | — | — |
| `/api/health` | readiness | — | — | — | — |
| `/api/engine/version` | version and protocol | — | — | — | — |
| `/api/engine/composition` | composition + surface, `ETag` | replace the document | — | — | — |
| `/api/engine/apps` | catalogue | — | — | — | install (id derived) |
| `/api/engine/apps/{id}` | descriptor, manifest, grant | replace the bundle | `enabled`, `alias` | uninstall | — |
| `/api/engine/apps/{id}/approval` | computed risk and levels | — | — | — | — |
| `/api/engine/apps/{id}/grant` | frozen grant | approve | — | revoke | — |
| `/api/engine/apps/{id}/wit` | installed WIT package | — | — | — | — |
| `/api/engine/audit` | paginated log | — | — | — | — |
| `/api/engine/keys` | trusted keys | — | — | — | add a key |
| `/api/engine/keys/distribution` | the compiled distribution key | — | — | — | — |
| `/api/engine/keys/{fingerprint}` | — | — | — | remove | — |
| `/api/engine/operator` | identity | — | change password | — | — |
| `/api/engine/session` | current session | — | — | log out | log in |
| `/api/engine/restart` | — | — | — | — | restart |
| `/api/apps/{id}/assets/{*module}` | a declared UI module | — | — | — | — |
| `/api/apps/{id}/{*path}` | per `[[route]]` | per `[[route]]` | per `[[route]]` | per `[[route]]` | per `[[route]]` |

### Composition

`GET` returns `{composition, surface}` with a strong `ETag` (the SHA-256 of the
canonical TOML). `PUT` replaces the whole document and requires `If-Match`; a
missing or stale validator is `412`, and `*` matches any existing document. The
body is the composition itself, not the view. The `surface` reports the resolved
apps, slots, CLI verbs, FAB actions, links, the degraded set and a fatal error
when the document cannot resolve.

### Apps

`POST /api/engine/apps` takes the raw bundle bytes (a zip), derives the id from
the manifest and answers `201` with `Location: /api/engine/apps/{id}`. The
second install of the same id is `409 conflict`.

`PUT /api/engine/apps/{id}` replaces the bundle; the id inside must match, the
old grant is revoked, and a declared authority delta is reported. `PATCH
/apps/{id}` takes `{"enabled": false}` or `{"alias": "notes"}` (an empty alias
clears the operator's alias). `DELETE` uninstalls; a breaking action returns
`409` with `detail.dependents` and `detail.acknowledge`, and the retry must send
`X-Anth-Acknowledge: dependents`.

### Approval and grants

`GET /api/engine/apps/{id}/approval` reports the computed risk, the required
levels and any authority diff. `PUT /api/engine/apps/{id}/grant` freezes it:

```json
{"accept": ["write", "admin"]}
```

The accepted levels must cover the risk (the engine says which level is
missing). `DELETE` revokes the grant and keeps the installed bytes.

### Keys and the distribution key

`GET /api/engine/keys` lists the keyring (`fingerprint`, `public_key`, `label`,
`added_by`, `added_at`); `POST` adds one (`public_key`, optional `label`); `DELETE
/api/engine/keys/{fingerprint}` removes it. The engine derives the fingerprint;
the public key is the request.

`GET /api/engine/keys/distribution` reports `{fingerprint, public_key, problem}`
for the key compiled into this build. It is public by definition: an operator
compares it with the release notes. `problem` is non-null only for a broken
build, which refuses installs.

### Audit

```http
GET /api/engine/audit?limit=100&before=…
```

Newest first. The response is `{entries, total, next, limit, before, problems}`;
`limit` is 1–1000, and `next` is the cursor for the previous page. Entries carry
`at`, `actor`, `action`, `app`, `result` and `detail`; denials are recorded even
when nothing changed.

### Restart

`POST /api/engine/restart` asks the host to replace its process. It answers `202
Accepted` with `Location: /api/health` before the new process is ready; poll the
readiness endpoint or `anth status`. A host that does not own its process answers
`503`. The action is audited as `engine.restart`.

## App routes

A request to `/api/apps/{id}/<path>` is matched against the app's `[[route]]`
entries by method and path. Only a running, approved app is reachable.

- `GET`, `HEAD` and `DELETE` take query parameters; other methods take a JSON
  object body keyed by the WIT parameter names.
- `OPTIONS` answers `204` with `Allow`; an undeclared method answers `405` with
  the list.
- A body on a no-body method is `400`.
- A `stream<list<u8>>` returns `application/octet-stream` chunked, a
  `stream<record>` returns `text/event-stream` (one JSON object per event), and a
  `future<T>` returns the value once. A stream that exhausts its budget ends with
  an `exhausted` error, never a silent truncation.
- A route declared with `transport = "websocket"` upgrades and streams frames.

The app's own UI never calls these routes directly from the panel origin: the
iframe bridge proxies them with the app's prefix ([Panels](panels.md)).

## Assets

`GET /api/apps/{id}/assets/{*module}` serves only a `slot.module` declared by a
running app's manifest and re-hashes it against the manifest. The response
carries `ETag: "<sha256>"` and `Cache-Control: immutable`; `If-None-Match`
answers `304` without reading the file. An undeclared path is `404`, and an
unverified asset is refused unless the development switch is on.

## Limits

`POST /apps` is capped by `[apps] max-bundle-bytes`; a login body is 16 KiB; a
route body is 1 MiB; an audit limit is 1000. Fuel, memory, wall time and stream
bytes apply per app and per call ([Apps](apps.md#limits)).
