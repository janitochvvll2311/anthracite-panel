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
- `429 rate_limited` after repeated attempts.

### `POST /api/auth/login`

Public.

```json
{ "username": "admin", "password": "..." }
```

- `200` with cookies and `{"token_type":"Bearer","expires_in":900}`.
- `401 invalid_credentials`.
- `409 not_registered` when no account exists.
- `429 rate_limited`.

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
