# Phase 0 contracts

Status: **accepted** for implementation. This document turns the v0.3.0
architecture draft into the contracts Phase 0 needs. Decisions below were
confirmed before code; later changes must update this file and the architecture
draft explicitly.

Implementation status: Phase 0 and the Phase 1 host-info slice have landed —
workspace, `anth-wire`, `anth-host`, config/state atomic writes, logger, CI,
Unix socket with `Hello`, HTTP + SSE, CLI `host-info`, minimal auth and the Lit
shell with its embed guard. Phase 2 adds login rate limiting, session listing
and revocation, the settings surface and restart/reboot. Phase 3 has started
with file listing, text and image previews, a tabbed create/edit/save editor,
a transfer queue for uploads and downloads, and deletion behind `files.write`,
including its web panel. The remaining feature phases are still pending.

Reference: [v0.3.0.md](v0.3.0.md).

## 0. Toolchain state

Installed under `/tools` so a recreated environment keeps them:

| Tool | Location | Version |
| --- | --- | --- |
| Rust | `/tools/cargo`, `/tools/rustup` | 1.99.1, target `x86_64-unknown-linux-musl`, `rustfmt`, `clippy` |
| Bun | `/tools/bun` | 1.4.4 |
| C toolchain | Alpine `gcc`, `musl-dev`, `make`, `pkgconf` | system packages |

- `/tools/env.sh` exports `CARGO_HOME`, `RUSTUP_HOME`, `BUN_INSTALL` and `PATH`.
- `/usr/local/bin/{cargo,rustc,rustup,rustfmt,cargo-fmt,cargo-clippy,bun}` are
  small wrappers that source `/tools/env.sh`, so a non-login shell works.
- `/etc/profile.d/tools.sh` and `/root/.profile`/`.bashrc` source the env file
  for login shells.
- The system C toolchain is not under `/tools`; a fully recreated image may need
  `apk add gcc musl-dev make pkgconf` again. The Rust/Bun toolchains survive.

**Comment:** the host tuple is musl because Alpine's rustup default target is
musl. That matches the project pin. A `rust-toolchain.toml` should still name
the channel and target explicitly in Phase 0 so the install is reproducible.

---

## 1. Wire v2 minimal contract

### Constants

```rust
pub const PROTOCOL_VERSION: u16 = 2;
pub const MAX_FRAME_LEN: usize = 64 * 1024 * 1024;
pub const DEFAULT_PAGE_LIMIT: u16 = 100;
pub const MAX_PAGE_LIMIT: u16 = 1000;
pub const MAX_CURSOR_LEN: usize = 512;
```

- Frame: 4-byte big-endian payload length, then JSON.
- A payload above `MAX_FRAME_LEN` is `TooLarge`, never shortened.
- Pre-1.0.0 breaks are allowed; bump `PROTOCOL_VERSION` and record the break in
  `protocol-changelog.md`.

### Handshake

```rust
Request::Hello {
    protocol_version: u16,
    client: String,          // "cli/0.3.0", "web/0.3.0", "test/0.3.0"
    features: Vec<String>,   // reserved; empty today
}

Response::Welcome {
    protocol_version: u16,
    server: String,          // "anth-panel"
    version: String,         // CARGO_PKG_VERSION
    features: Vec<String>,   // empty today
}
```

- `Hello` is mandatory as the first socket frame.
- `client` is non-empty, printable, at most 64 characters; no path or secret.
- `features` entries are at most 64 characters, at most 32 entries; unknown
  features are ignored. This is the scaffold for post-1.0 negotiation.
- A mismatch answers `Invalid` with code `protocol_mismatch`, names both
  versions, then closes the connection.
- HTTP sends `X-Anth-Protocol: 2` on responses. A missing request header is
  accepted; a present mismatching one is `400 invalid_request`. This is a
  scaffold, not a compatibility promise.

### Core requests and responses

Phase 0/1 only:

```rust
Request::HostInfo
Response::HostInfo(HostInfo)
Response::Error(WireError)
```

`Echo` is deliberately not added: `HostInfo` is the round-trip proof and a
second diagnostic command has no consumer.

### `HostInfo`

```rust
pub struct HostInfo {
    pub hostname: String,
    pub os: OsInfo,
    pub uptime_seconds: u64,
    pub cpu_model: String,
    pub logical_cores: u32,
    pub load: LoadAverage,
    pub memory: MemoryInfo,
    pub collected_at_ms: u64,
}

pub struct OsInfo {
    pub name: String,
    pub version: String,
    pub kernel: String,
    pub arch: String,
}

pub struct LoadAverage {
    pub one: f64,
    pub five: f64,
    pub fifteen: f64,
}

pub struct MemoryInfo {
    pub total_kib: u64,
    pub available_kib: u64,
    pub swap_total_kib: u64,
    pub swap_free_kib: u64,
}
```

- All widths are fixed (`u32`/`u64`), never `usize`, so the wire does not depend
  on the host architecture.
- Unreadable fields degrade to `"unknown"` or `0`; the request still succeeds.
- `collected_at_ms` is Unix epoch milliseconds; it is the SSE event id and lets
  the UI show freshness.
- `used` memory and swap are computed in the client from the four counters; the
  wire keeps the raw values.

### Pagination

```rust
pub struct PageRequest {
    pub limit: Option<u16>,
    pub cursor: Option<String>,
}

pub struct Page<T> {
    pub items: Vec<T>,
    pub next: Option<String>,
    pub total: Option<u64>,
}
```

- Each list response carries `Page<T>` in its own variant; there is no generic
  `Response::Page` whose `T` is erased.
- `limit` defaults to 100 and is capped at 1000. `0` is invalid.
- `next = None` is the only end signal. An empty page is valid.
- `cursor` is opaque base64url, at most 512 characters. It encodes the feature,
  protocol version and the last stable key. A cursor from another feature or
  version is `Invalid`, not an empty page.
- Ordering is documented per feature. A cursor is only valid for that ordering.
- `total` is optional and only returned when it is cheap to compute.
- File reads keep `offset`/`next`; that is a byte cursor, not a handle.

**Comment:** `Page<T>` costs one extra struct per list response. That is
preferred over a generic wire variant because serde and the TypeScript mirror
stay concrete and readable. The first real list feature will prove the cursor
shape; the constants exist now so no one invents a truncation pass.

### Errors

```rust
pub enum ErrorKind {
    NotFound, Forbidden, Unauthorized, Conflict,
    Invalid, Unsupported, TooLarge, TooMany, Internal,
}

pub struct WireError {
    pub kind: ErrorKind,
    pub code: String,     // stable snake_case, e.g. "policy_off"
    pub message: String,  // human-readable
}
```

- HTTP status and CLI exit code derive from `kind` in one place.
- `code` is the stable machine tag; `message` is not for branching.
- Phase 0/1 codes: `protocol_mismatch`, `frame_too_large`, `invalid_hello`,
  `invalid_request`, `auth_required`, `invalid_credentials`, `bootstrap_closed`,
  `rate_limited`, `password_too_short`, `internal`, `unsupported`.
- Default code per kind: `not_found`, `forbidden`, `unauthorized`, `conflict`,
  `invalid`, `unsupported`, `too_large`, `too_many`, `internal`.
- `TooMany` is HTTP `429` and exit `1`; it is the rate-limit kind.
- HTTP body:
  `{"error":{"kind":"forbidden","code":"policy_off","message":"..."}}`.
- A plugin's own non-2xx result is a successful call, not a transport error.

**Comment:** `code` is a `String` rather than `&'static str` so a feature can
add a meaningful code without growing a global enum. The kind stays closed.

---

## 2. Config, secrets, sessions and state

### Path precedence

1. CLI flag.
2. Environment variable.
3. `config.toml`.
4. Default.

Proposed defaults:

| Thing | Release | Debug (`cfg!(debug_assertions)`) |
| --- | --- | --- |
| config | `/etc/anth/config.toml` | `./config.toml` |
| socket | `/var/tmp/anth.sock` | `./anth.sock` |
| state dir | `/var/lib/anth` | `./state` |
| plugin root | `/var/lib/anth/plugins` | `./plugins` |
| grants | `<config_dir>/plugins.d` | `./plugins.d` |
| secrets | `<config_dir>/secrets.toml` | `./secrets.toml` |

Env names: `ANTH_CONFIG`, `ANTH_SOCKET`, `ANTH_STATE`, `ANTH_PLUGINS`,
`ANTH_PANEL_ADDR`, `ANTH_LOG_LEVEL`, `ANTH_LOG_FORMAT`, `ANTH_LOG_OUT`.

Relative paths inside `config.toml` resolve against the config file's directory
(not the process CWD). CLI and environment overrides stay as given.

**Accepted:** debug uses relative defaults (`./config.toml`, `./anth.sock`,
`./state`, `./plugins`, `./plugins.d`, `./secrets.toml`), so `cargo run` needs no
root or flags. Release uses `/etc/anth/*` and `/var/lib/anth/*` as in the table.

### `config.toml` v1

```toml
version = 1
socket_path = "/var/tmp/anth.sock"
state_dir = "/var/lib/anth"

[server]
addr = "0.0.0.0:8080"

[log]
level = "info"
format = "human"
out = "stdout"

[terminal]
enabled = false
shell = "auto"
max_sessions = 4
idle_seconds = 900

[process]
control = false
spawn = false
spawn_allow = []

[network]
control = false
manage = false

[firewall]
control = false

[volumes]
control = false
partition = false
format = false
mount_root = "/mnt"
disk_allow = []

[accounts]
control = false
manage_passwords = false
allow_system = false
allow_privileged_groups = false
home_root = "/home"
shells = []
default_shell = "/bin/sh"

[files]
root = "/"
write = false
trash = false
max_entries = 5000
max_preview_bytes = 1048576
max_upload_bytes = 67108864
max_archive_bytes = 4294967296

[auth]
enabled = true
username = "admin"
access_token_secs = 900
refresh_token_secs = 2592000
cookie_secure = false
bootstrap = "auto"          # auto | generated | web

[plugins]
enabled = false
root = "/var/lib/anth/plugins"
install_from_web = false
require_signature = true
public_key = ""
fuel = 50000000
```

- `#[serde(default, deny_unknown_fields)]` on every section.
- `version` is mandatory. Missing `version` is a legacy v0.2 file and goes
  through `migrate`; an unknown version is refused.
- `bootstrap = "auto"` resolves to `web` in debug and `generated` in release.
- `username` stays in the config because it is not a secret and the web
  bootstrap uses it. The password hash and signing key are in `secrets.toml`.
- `iterations` is not a config key in v0.3. Argon2 parameters travel in the PHC
  string.

### `secrets.toml` v1 (`0600`)

```toml
version = 1

[auth]
password_hash = "$argon2id$v=19$m=19456,t=2,p=1$..."
signing_key = "64-hex-characters"
```

- No username, no plaintext password.
- `password_hash` empty or absent means the account has no password yet; that
  is the bootstrap-pending state.
- `signing_key` is generated independently; an existing key is never replaced
  by a password change.

### `sessions.toml` v1 (`0600`)

```toml
version = 1

[[session]]
digest = "sha256-hex"
chain = "random-hex"
expires = 1790000000
used = false
```

- At most 10 live records; expired records are pruned on write.
- `digest` is SHA-256 of the opaque refresh token; the token itself is never
  stored.
- `chain` identifies one login and its rotations; a reused consumed token
  removes the whole chain.
- `used = true` records a consumed token until expiry so reuse can be detected.
- Writes are atomic and serialized by an in-process lock plus an advisory
  `flock` on a sibling lock file.

### `plugins.state.toml` v1 (`0600`)

```toml
version = 1
disabled = ["org.example.plugin"]
```

- Absence or an empty list means every installed plugin is enabled.
- The grant schema for `plugins.d/<id>.toml` is finalized in Phase 7; the path,
  mode and "one file per plugin" rule are fixed now.

### Atomic writes that preserve comments

This applies to **every** user-editable file: `config.toml`, `secrets.toml`,
`sessions.toml`, `plugins.state.toml`, and later grants.

- Read the file as text. If missing, start from a documented template.
- Parse with `toml_edit::DocumentMut`. Never serialize the runtime struct back
  over a user-edited file.
- Mutate only the named keys/tables.
- Write through `write_atomic`: temp file in the same directory, mode set at
  creation, `write_all`, `flush`, `fsync`, `rename`, `fsync` the directory.
- On any failure, remove the temp and leave the original byte-for-byte.
- A parse error is refused with the path and parser message; never replaced
  with defaults.
- Serialize the read-modify-write with an in-process mutex and an advisory
  `flock` on `<file>.lock` (`0600`) so `config set` from the CLI and a running
  daemon cannot lose each other's edits.
- `config.toml` is mode `0644`; files with credentials or session material are
  `0600`; the state directory is `0700`.

**Comment:** `toml_edit` is the only way to keep comments and key order. The
runtime may still parse the resulting text with `toml` + serde. The two paths
must share one schema, so the writer edits by key name and a test asserts that
the written text parses into the same struct.

---

## 3. Bootstrap flow (mixed mode)

### Key

```toml
[auth]
bootstrap = "auto"   # auto | generated | web
```

- `auto`: debug build -> `web`; release build -> `generated`.
- `generated`: generate a random password, print it once, store only the hash.
- `web`: expose the registration endpoint/form while no password is set.

### State machine

Bootstrap applies only when `auth.enabled = true` and the account is
incomplete:

| Account state | `generated` | `web` |
| --- | --- | --- |
| no `secrets.toml` / no `password_hash` | generate password, write hash + signing key, print once, close bootstrap | keep `password_hash` empty; expose `POST /api/auth/register`; generate signing key; `bootstrap` state is open |
| `password_hash` present | do nothing; normal login | do nothing; normal login |
| `auth.enabled = false` | no bootstrap, no account created; startup warns | same |

- The web route closes permanently after the first successful registration.
  The check and write happen under the same file lock, so two concurrent
  requests cannot both register.
- In `web` mode, the username comes from `[auth] username`; the form asks for a
  password and confirmation, not a new username. This keeps one operator
  identity and one place for it.
- In `generated` mode, the password is 20 characters, printed to **stderr**
  once, never to the structured log, never to a file. The log says only that a
  password was generated and where it was printed.
- If `generated` runs without a usable stderr (supervisor, redirected), the
  operator can still set a password with `anth-panel auth set-password`.
- `web` in a release build is allowed only when explicitly configured, and
  startup logs a warning that registration is open. It is not the default.
- `auth.enabled = false` disables the API gate but does not delete an existing
  account, hash or signing key.

### `/api/auth/state`

```json
{
  "required": true,
  "registered": false,
  "bootstrap": "web",
  "username": "admin"
}
```

- `required`: the auth layer is enabled.
- `registered`: a password hash exists.
- `bootstrap`: `"web"` while registration is open, `"closed"` otherwise.
  `generated` is resolved at startup, so it is not an HTTP state.
- The frontend shows the registration form only for `bootstrap = "web"` and
  `registered = false`, and the login form otherwise.

**Accepted:** the generated password prints to stderr (not stdout), and web
registration asks only for a password; the username comes from `[auth] username`.

---

## 4. HTTP and CLI for the first vertical

### HTTP

| Method | Path | Auth | Purpose |
| --- | --- | --- | --- |
| GET | `/healthz` | public | constant `{"status":"ok"}`, no config read |
| GET | `/api/auth/state` | public | bootstrap/login gate state |
| POST | `/api/auth/register` | public only while web bootstrap is open | `{password}` -> 201 + cookies |
| POST | `/api/auth/login` | public | `{username,password}` -> 200 + cookies |
| POST | `/api/auth/refresh` | refresh cookie/body | rotate refresh, new access |
| POST | `/api/auth/logout` | access or refresh | clear cookies; revoke session(s) |
| POST | `/api/auth/password` | access | `{current_password,new_password}`; revoke all |
| GET | `/api/host-info` | access | `HostInfo` |
| GET | `/api/host-info/watch?interval=2s` | access | SSE, one `HostInfo` per tick |

- All `/api` responses carry `X-Anth-Protocol: 2`.
- Error body is the `WireError` JSON above.
- `register` is `409 bootstrap_closed` once closed; `429 rate_limited` on the
  login/register limit; `400 password_too_short` below 12 characters.
- `/healthz` is public even when auth is on.
- Static assets and `/` are public; they are the login page.
- `GET /api/host-info/watch` validates `interval` before the stream opens and
  answers `400` on a bad value. Events: `event: host-info`, `id` =
  `collected_at_ms`, `data` = `HostInfo`; `retry: 3000`; keep-alive every 15s.
  A failure after the first byte is `event: error` with the `WireError` JSON and
  closes the stream.
- No `Last-Event-ID` replay: each tick is a fresh snapshot; a reconnecting
  browser starts with the next tick.

**Comment:** this moves a minimal auth slice into Phase 1. Without it, the first
HTTP vertical would either be open by default or need a throwaway `--no-auth`
switch. Auth is part of the first shippable slice; lifecycle and settings stay
in Phase 2.

### CLI

```text
anth-panel [--config FILE] [--socket PATH] [--log-*] <command>

run [--addr ADDR]              # daemon + HTTP in one process
daemon                         # socket only
host-info [--watch [INTERVAL]]
auth status|register|set-password
config get|set|validate
migrate --from FILE [--write]
```

- `run` is the first vertical; `daemon` is the socket-only counterpart. `api`
  as a separate process is deferred until a real second-process consumer exists.
- Global flags: `--config`, `--socket`, `--log-format`, `--log-level`,
  `--log-out`. `--interactive` comes with the REPL phase, not Phase 1.
- Exit codes: `0` success, `1` runtime/daemon/transport error, `2` usage or
  invalid request (derived from `ErrorKind::Invalid`).
- `host-info` replaces the dense table with grouped sections that mirror the
  web card, without a border and without a label for every field:

  ```text
  Anthracite Panel · host.example
  Debian 13 · kernel 6.12.111 · x86_64 · up 3d 4h

  CPU
    AMD EPYC 7B13
    8 logical cores · load 0.12 0.20 0.31

  Memory
    3.4 GiB / 7.6 GiB used (45%) · 4.2 GiB available

  Swap
    not configured
  ```

  The sections are Host/OS, CPU + load, Memory and Swap. The exact spacing and
  wording are pinned by render tests; the shape stays stable for `--watch`.
- `host-info --watch`: one connection; on a TTY overwrite the frame, when
  redirected separate frames with a blank line; `Ctrl-C` exits `0`; a dropped
  daemon exits `1`.
- `auth register` reads the password from the TTY (or `--stdin`), writes
  `secrets.toml` atomically, and closes the web bootstrap.
- `config get` prints TOML; `config set KEY VALUE` edits only that key and
  preserves comments; `config validate` parses without writing.
- `migrate` is dry-run unless `--write`; it never overwrites the source.

**Accepted:** the grouped `host-info` layout above; the separate `api` process
is deferred until a real second-process consumer exists.

---

## 5. Observability and error taxonomy

### Log settings

Resolution: CLI flag > `ANTH_LOG_*` > `[log]` > default.

| Setting | Values | Default |
| --- | --- | --- |
| level | `error,warn,info,debug,trace` | `info` |
| format | `human,json,off` | `human` |
| out | `stdout,stderr` | `stdout` |

- Human: `STAMP LEVEL target: message key=value ...`.
- JSON: one object per line with `ts` (RFC 3339 UTC ms), `level`, `target`,
  `msg`, then typed fields.
- The logger is passed as `Arc<Logger>`, never installed globally.
- A guest log line is escaped and capped at 64 KiB per invocation; the guest
  cannot raise its own level.
- Never log passwords, tokens, signing keys, private keys, cookies,
  authorization headers or full file bodies. A host job that handles a secret
  returns a redacted result.
- Startup logs: version, config path, socket path, state dir, auth mode,
  bootstrap mode, enabled policy switches. No secret values.
- Request logs: `op`, `status`, `ms`; failures add `error_kind` and `error_code`.
  `/healthz` is logged at `debug` only.

**Comment:** no request id in Phase 0. Add one when there is a second
concurrent subsystem that needs correlation; a request id without a consumer is
the speculative abstraction the project avoids.

### Error codes

The `code` is the stable machine tag. The initial registry:

| Code | Kind | When |
| --- | --- | --- |
| `protocol_mismatch` | Invalid | `Hello` version differs |
| `invalid_hello` | Invalid | malformed/missing first frame |
| `frame_too_large` | TooLarge | frame over 64 MiB |
| `invalid_request` | Invalid | malformed body/enum |
| `auth_required` | Unauthorized | no session |
| `invalid_credentials` | Unauthorized | bad login |
| `bootstrap_closed` | Conflict | register after close |
| `rate_limited` | TooMany | login/register limit exceeded |
| `password_too_short` | Invalid | password < 12 chars |
| `not_found` | NotFound | missing resource |
| `forbidden` | Forbidden | policy/guard refusal |
| `conflict` | Conflict | state conflict |
| `unsupported` | Unsupported | missing host tool/feature |
| `too_large` | TooLarge | payload/limit |
| `internal` | Internal | unclassified |

**Accepted:** `TooMany` is a real `ErrorKind` with HTTP `429`. It is distinct
from `Conflict`; the UI backs off and honours `Retry-After`.

---

## 6. Temporary directories

Use `/tmp/anthracite-panel`, not the shared `/tmp` root and not `.tmp`:

```
/tmp/anthracite-panel/
  tests/<name>-<pid>-<nanos>/
  run/
    daemon.pid daemon.log anth.sock
    vite.pid vite.log
```

- Root mode `0700`; create it on demand.
- Tests set `TMPDIR=/tmp/anthracite-panel/tests` (or call a shared helper that
  creates a unique subdirectory). Never `rm -rf` the root; remove only the
  test's own directory.
- Manual harnesses use `/tmp/anthracite-panel/run`.
- The path is environment-specific, so do not bake it into config defaults.

---

## 7. Git and PR format

Keep the v0.2 shape:

- Integration branch: `v0.3.0` (the current rewrite branch).
- Feature branches: `feat/<phase>-<slug>`, e.g. `feat/0-foundation`,
  `feat/1-host-info`. Branch from `v0.3.0`; PR target `v0.3.0`.
- One PR per main phase, or per independent point inside a phase when it can
  merge and be tested alone. Do not mix unrelated phases.
- Commit message: `area: short imperative sentence`, matching v0.2
  (`panel: ...`, `files: ...`, `docs: ...`). Use `feat:`/`fix:` only when the
  area prefix would be misleading.
- PR body: Summary, Scope, Tests, Security/Policy, Docs, Open items.
- Merge with a merge commit (preserve the phase history), not a squash, unless
  the branch is a single trivial docs commit.
- No direct pushes to `v0.3.0`; every change goes through a PR.
- Release tags: `v0.3.0-rc.N` for candidates, `v0.3.0` for the release. The old
  `main` tree stays untouched until the rewrite is ready to become `main`.
- Review: at least one review before merge. The v0.2 "wait up to two hours"
  rule is not a code rule; keep it as a team practice only if you want it.

**Accepted:** branch base and target are `v0.3.0`, not `main` (main still holds
v0.1). This is the one place where "same as v0.2" cannot be literal.

---

## 8. Approved decisions

1. **Wire:** `Hello`/`Welcome`, `HostInfo` and per-response `Page<T>` as above;
   `collected_at_ms` is included.
2. **Paths:** debug uses relative `./*` defaults; release uses `/etc/anth/*` and
   `/var/lib/anth/*`.
3. **Bootstrap:** `auto`/`generated`/`web`; generated prints to stderr;
   web registration asks only for a password; username comes from config.
4. **Auth kind:** `TooMany` exists and maps to HTTP `429`.
5. **CLI:** grouped, webui-like `host-info` sections; the separate `api` process
   is deferred.
6. **Git:** feature branches from `v0.3.0`, PRs per phase or independent point.
7. **Temp:** `/tmp/anthracite-panel` is the scratch root.
