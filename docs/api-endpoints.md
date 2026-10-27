# HTTP API

The panel serves JSON over HTTP and SSE. The same `anth-panel` process can also
serve the Unix socket.

## Conventions

- Routes name the command; bodies carry the command's arguments at the top
  level, with no socket `type`/`data` envelope.
- Every API response carries `X-Anth-Protocol: 2`.
- Errors use one shape:

  ```json
  {
    "error": {
      "kind": "forbidden",
      "code": "policy_off",
      "message": "process control is disabled"
    }
  }
  ```

  `kind` is one of `not_found`, `forbidden`, `unauthorized`, `conflict`,
  `invalid`, `unsupported`, `too_large`, `too_many`, `internal`.
- The access token lives in the `anth_access` cookie or an
  `Authorization: Bearer` header. The refresh token lives in `anth_refresh`,
  scoped to `/api/auth`.
- Mutating auth requests also check `Origin` against `Host`.
- An unknown `/api` path is a JSON `404`, never the SPA.

## `GET /healthz`

Public. Returns `{"status":"ok"}`. It does not read config or credentials.

## Authentication

### `GET /api/auth/state`

Public. The shell calls it before rendering a form.

```json
{
  "required": true,
  "registered": true,
  "authenticated": false,
  "bootstrap": "closed",
  "username": "admin"
}
```

- `required`: auth is enabled.
- `registered`: a password hash exists.
- `authenticated`: the request carried a live access token.
- `bootstrap`: `web` while first-run registration is open, otherwise `closed`.

### `POST /api/auth/register`

Public only while `bootstrap = "web"` and no password exists.

```json
{ "password": "at least 12 characters" }
```

- `201` with the access/refresh cookies, or `409 bootstrap_closed` after the
  first registration.
- `400 password_too_short` when the password is under 12 characters.
- `429 too_many` after repeated failures, with `Retry-After` in seconds.

### `POST /api/auth/login`

Public.

```json
{ "username": "admin", "password": "..." }
```

- `200` with cookies and `{"token_type":"Bearer","expires_in":900}`.
- `401 invalid_credentials`.
- `409 not_registered` when no account exists.
- `429 too_many` after five failures from one source in a minute, with
  `Retry-After` in seconds. Only failures spend the budget; a successful login
  clears it.

### `POST /api/auth/refresh`

Uses the `anth_refresh` cookie, or `{ "refresh_token": "..." }` in the body.

- `200` with a new cookie pair.
- `401 invalid_credentials` when the token is unknown, expired or reused.
- A reused token drops every session in its chain.

### `POST /api/auth/logout`

Clears the cookies and drops the operator's sessions. Returns `{"ok":true}`.

### `POST /api/auth/password`

Requires a live access token.

```json
{ "current_password": "...", "new_password": "..." }
```

- `200` and clears the cookies; every session is revoked.
- `401` if the current password is wrong.
- `400 password_too_short`.

### `GET /api/auth/sessions`

Requires a live access token.

```json
{
  "max": 10,
  "sessions": [
    { "sid": "d301d3dab719f6803488bfbec29d8483", "expires": 1795634191, "current": true }
  ]
}
```

One entry per login, not per refresh rotation. `sid` is safe to display: the
refresh token is stored only as a digest. `expires` is Unix seconds.

### `POST /api/auth/sessions/revoke`

Requires a live access token.

```json
{ "others": true }
```

- `{"ok":true,"revoked":1,"scope":"others"}` revokes every session except the
  caller's.
- `{ "sid": "..." }` revokes one session instead.

## Host info

### `GET /api/host-info`

Requires a live access token unless auth is disabled.

```json
{
  "hostname": "example",
  "os": { "name": "Debian GNU/Linux", "version": "13", "kernel": "6.12.0", "arch": "x86_64" },
  "uptime_seconds": 1234,
  "cpu_model": "Example CPU",
  "logical_cores": 8,
  "load": { "one": 0.12, "five": 0.2, "fifteen": 0.31 },
  "memory": {
    "total_kib": 8388608,
    "available_kib": 4194304,
    "swap_total_kib": 0,
    "swap_free_kib": 0
  },
  "collected_at_ms": 1790000000000
}
```

Unreadable fields degrade to `unknown` or `0`.

### `GET /api/host-info/watch?interval=2s`

Requires a live access token. Server-Sent Events:

- `event: host-info`
- `id`: `collected_at_ms`
- `data`: the same `HostInfo` object
- `retry: 3000`
- Keep-alive comments every 15 seconds.

A bad interval is a `400` before the stream opens. After the first byte, a
failure is an `event: error` with the error JSON and the stream closes.
`interval` accepts `ms`, `s`, `m` and `h` suffixes and is at least `10ms`.

## Settings and lifecycle

All four routes require a live access token unless auth is disabled. The two
mutations check `Origin` against `Host`.

### `GET /api/settings`

Returns the operator policy as it is in `config.toml` (the field names match the
file). It contains no secrets: credentials live in `secrets.toml`.

### `PATCH /api/settings`

```json
{ "key": "auth.username", "value": "operator" }
```

Changes one dotted key through the same comment-preserving editor as
`config set`, validates the whole file, and writes it atomically. The daemon
keeps its current policy until a restart.

- `200 {"ok":true,"key":"auth.username","restart_required":true}`
- `400` for an unknown key, a value of the wrong type, or a file that would no
  longer parse.

### `POST /api/restart`

```json
{ "restarting": true, "pid": 1234 }
```

`202 Accepted`. The daemon answers first, then replaces itself with a fresh copy
of the same command line, which re-reads `config.toml`. The pid is kept.

### `POST /api/reboot`

```json
{ "delay_s": 0 }
```

`202 {"rebooting":true,"delay_s":0}`. The delay is at most 300 seconds. The
daemon refuses with `403 forbidden` unless it was started with `--allow-reboot`.

## Files

The routes require a live access token unless auth is disabled. Paths are
relative to `[files] root`; `..` and paths that escape through a symlink are
refused.

### `GET /api/files?path=/&limit=100&cursor=...`

```json
{
  "items": [
    { "name": "etc", "kind": "dir", "size": 0, "modified_ms": 1790000000000 },
    { "name": "hosts", "kind": "file", "size": 220, "modified_ms": 1790000000000 }
  ],
  "next": "hosts"
}
```

`kind` is `file`, `dir`, `symlink` or `other`. `next` is absent on the last
page. The listing is sorted by name; `cursor` is the previous `next` value.

### `GET /api/files/read?path=etc/hosts&offset=0&max_bytes=...`

```json
{
  "path": "etc/hosts",
  "offset": 0,
  "size": 220,
  "eof": true,
  "text": "127.0.0.1 localhost\n"
}
```

Only regular UTF-8 text files are accepted. `max_bytes` is capped by
`files.max_preview_bytes`; `eof` tells the caller whether another chunk exists.

### `GET /api/files/image?path=img/logo.png`

Returns the raw bytes for the image preview. Supported types are `png`, `jpg`,
`jpeg`, `gif`, `webp`, `bmp`, `avif`, `ico` and `svg`; the response carries the
matching `Content-Type`, `Cache-Control: no-store`, `X-Content-Type-Options:
nosniff` and a sandboxing CSP. The size is capped at the smaller of
`files.max_upload_bytes` and 32 MiB, and the same root containment rules apply.
This is an HTTP-only binary route; the socket remains JSON.

### `GET /api/files/download?path=etc/hosts`

Returns the raw bytes of any regular file as `application/octet-stream` with
`Content-Disposition: attachment`, `Cache-Control: no-store` and `nosniff`. The
same root containment rules apply and the size is capped by
`files.max_upload_bytes`. The web UI uses it for the transfer queue and names
the saved file after the request path.

### `DELETE /api/files?path=etc/hosts`

Deletes one regular file or a symlink (the link itself, not its target). The
daemon answers with 200:

```json
{ "path": "etc/hosts" }
```

Deletion requires `files.write = true`; otherwise the daemon answers `403
forbidden` with code `write_disabled`. Directories are `400 invalid`, and the
file root cannot be deleted. This is a permanent delete; trash and archives are
part of the next files slice, so the caller confirms before sending the
request.

### `POST /api/files/write`

```json
{ "path": "etc/motd", "content": "hello\n", "create": false }
```

Writes one UTF-8 text file and answers:

```json
{ "path": "etc/motd", "size": 6 }
```

`create: true` fails with `409 conflict` (code `exists`) instead of replacing an
existing file; the default replaces it. Writing requires `files.write = true`
(`403` with `write_disabled` otherwise). The content is capped by
`files.max_upload_bytes`; directories and non-regular files are refused, and
symlinks that leave the root are rejected. The request body limit follows
`files.max_upload_bytes` plus room for the JSON envelope.

### `POST /api/files/upload`

```json
{ "path": "img/logo.png", "content_base64": "iVBORw0KGgo=", "create": true }
```

Writes arbitrary bytes decoded from base64 and answers like the text write:

```json
{ "path": "img/logo.png", "size": 8 }
```

It has the same `files.write` gate, `create`/`exists` behavior, size cap and
path guards as the text write. The body limit allows for the base64 expansion
of `files.max_upload_bytes`.

## Static UI

`GET /` and `GET /{*path}` serve the embedded shell. A missing file is `404`; an
unknown browser route falls back to `index.html`. `/api` never falls back to the
HTML shell.

## Status codes

| Kind | HTTP |
| --- | --- |
| `not_found` | 404 |
| `forbidden` | 403 |
| `unauthorized` | 401 |
| `conflict` | 409 |
| `invalid` | 400 |
| `unsupported` | 501 |
| `too_large` | 413 |
| `too_many` | 429 |
| `internal` | 500 |
