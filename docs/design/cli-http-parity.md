# CLI and HTTP parity

HTTP is the one public dialect. The daemon serves the same router on TCP and on
the Unix socket; the CLI is an HTTP client (`anth-client`) over that socket, and
the browser uses `fetch` and `EventSource`. There is no second request
vocabulary to keep in sync, so this document is now an inventory instead of a
two-transport audit.

The rule is still functional parity:

- A host operation the daemon can perform has an HTTP route and a CLI command.
- Streaming uses the HTTP forms the browser already uses: SSE for snapshots and
  a WebSocket for the terminal. A CLI command that needs the same data uses the
  same route.
- Local CLI utilities (`config get`, `config validate`, `run`, `daemon`) act on
  the local file or process rather than on the daemon, so they have no route.
  `config show` mirrors `GET /api/settings`.

Local requests are authenticated by `peer_cred`; the listener installs a local
owner and the CLI needs no login. Remote HTTP requests use a cookie (browser) or
a bearer token (custom client).

## Feature matrix

### Host, lifecycle and settings

| Operation | HTTP | CLI |
| --- | --- | --- |
| Handshake and protocol version | `GET /api/hello` | every command checks it |
| Host snapshot | `GET /api/host-info` | `host-info` |
| Host stream | `GET /api/host-info/watch` (SSE) | `host-info --watch` |
| Restart | `POST /api/restart` | `restart` |
| Reboot | `POST /api/reboot` | `reboot --delay` |
| Effective settings | `GET /api/settings` | `config show` |
| Change one setting | `PATCH /api/settings` | `config set` |
| Raw config file | — | `config get` |
| Validate config | — | `config validate` |
| App catalogue and install | `GET /api/apps`, `POST /api/apps/install` | `app list`, `app install` |
| Enable/disable/approve/remove an app | `POST /api/apps/{id}/…` | `app enable/disable/approve/remove` |
| App anchor | `/api/apps/{id}/{*path}` | `app call` |
| Declared app UI asset | `GET /api/apps/{id}/web/{*path}`, `GET /app/{id}/{*module}` | — (browser only) |

### Files

| Operation | HTTP | CLI |
| --- | --- | --- |
| List directory | `GET /api/files` | `files list` |
| Read UTF-8 text | `GET /api/files/read` | `files read` |
| Read arbitrary bytes | `GET /api/files/download` | `files download` |
| Image preview | `GET /api/files/image` | `files download` |
| Write UTF-8 text | `POST /api/files/write` | `files write` |
| Write arbitrary bytes | `POST /api/files/upload` | `files upload` |
| Delete | `DELETE /api/files` | `files delete` |
| Trash | `POST /api/files/trash` | `files trash` |
| Trash list | `GET /api/files/trash/list` | `files trash-bin list` |
| Trash restore | `POST /api/files/trash/restore` | `files trash-bin restore` |
| Trash empty | `POST /api/files/trash/empty` | `files trash-bin empty` |
| Create archive | `POST /api/files/archive` | `files archive` |
| List archive | `GET /api/files/archive/list` | `files unarchive --list` |
| Extract archive | `POST /api/files/archive/extract` | `files unarchive` |

`files download` copies the response body straight to stdout, so binary files
and large files do not pass through JSON or base64. The HTTP `image` route is
the browser preview wrapper: it validates the image type, adds a MIME header and
a restrictive CSP. Both read through the same `FileService` guards, including
`files.max_upload_bytes`.

### Network and firewall

| Operation | HTTP | CLI |
| --- | --- | --- |
| Interface snapshot | `GET /api/network` | `network list` |
| Interface stream | `GET /api/network/watch` (SSE) | `network list --watch` |
| Up/down | `POST /api/network/state` | `network up/down` |
| Create | `POST /api/network/create` | `network create` |
| Edit | `POST /api/network/edit` | `network edit` |
| Rename | `POST /api/network/rename` | `network rename` |
| Delete | `POST /api/network/delete` | `network delete` |
| Firewall snapshot | `GET /api/firewall` | `firewall status` |
| Add allow/deny | `POST /api/firewall/rule` | `firewall allow/deny` |
| Edit rule | `POST /api/firewall/update` | `firewall edit` |
| Delete rule | `POST /api/firewall/delete` | `firewall delete` |
| Flush chain | `POST /api/firewall/flush` | `firewall flush` |
| Chain policy | `POST /api/firewall/policy` | `firewall policy` |

### Storage and accounts

| Operation | HTTP | CLI |
| --- | --- | --- |
| Storage snapshot | `GET /api/volumes` | `storage list` |
| Mount | `POST /api/volumes/mount` | `storage mount` |
| Unmount | `POST /api/volumes/unmount` | `storage unmount` |
| Create partition | `POST /api/volumes/partition/create` | `storage partition create` |
| Resize partition | `POST /api/volumes/partition/resize` | `storage partition resize` |
| Delete partition | `POST /api/volumes/partition/delete` | `storage partition delete` |
| Format | `POST /api/volumes/format` | `storage format` |
| Account snapshot | `GET /api/accounts` | `account list` |
| Create user | `POST /api/accounts/users` | `account create` |
| Delete user | `POST /api/accounts/users/delete` | `account delete` |
| Set password | `POST /api/accounts/users/password` | `account password` |
| Create group | `POST /api/accounts/groups` | `account group create` |
| Delete group | `POST /api/accounts/groups/delete` | `account group delete` |

### Processes and terminal

| Operation | HTTP | CLI |
| --- | --- | --- |
| Process page | `GET /api/processes` | `process list` |
| Process stream | `GET /api/processes/watch` (SSE) | `process list --watch` |
| Signal | `POST /api/processes/signal` | `process signal` |
| Spawn | `POST /api/processes/spawn` | `process spawn` |
| Terminal | `GET /api/terminal` (WebSocket) | `terminal` |

## Routes without a CLI twin

| Route | Why it has no CLI |
| --- | --- |
| `GET /healthz` | Process liveness probe for containers and load balancers |
| `GET /` and `GET /{path}` | Embedded web shell and static assets |
| `GET /api/auth/state` | Browser login/registration state |
| `POST /api/auth/register`, `login`, `refresh`, `logout` | Browser session lifecycle; the CLI is authenticated by Unix-socket peer credentials |
| `POST /api/auth/password` | Changes the panel password and revokes other web sessions |
| `GET /api/auth/sessions`, `POST /api/auth/sessions/revoke` | Web-session management for the settings page |
| `GET /api/files/image` | Browser preview with MIME detection and CSP; `files download` returns the same bytes |
| `GET /api/demo`, `POST /api/demo/reset` | Hosted-demo session status and reset; there is no CLI demo session to reset |
| SSE `*/watch` routes | Streaming form of a snapshot; the equivalent CLI commands use the same routes |

## Keeping parity

When a feature adds an operation:

1. Add the dispatch arm and the operation name in `dispatch.rs`.
2. Add the HTTP route, its request body and its response DTO in `anth-wire`.
3. Add the typed method to `anth-client` (Rust and TypeScript).
4. Add the CLI branch that calls the method.
5. Add a test through the router, one through the local socket, and one that
   asserts the CLI output or exit code.
6. Add the row to this matrix. If the route is transport-only, add it to the
   exception table with the reason.
