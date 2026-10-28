# CLI and HTTP parity

The socket contract is the common layer: both the CLI and the HTTP adapter build
the same `anth_wire::Request` and read the same `Response`. This document is the
audit that keeps one transport from growing an operation the other cannot reach.

The rule is functional parity, not identical presentation:

- A host operation that exists as a wire request must have a CLI command and an
  HTTP route (or a deliberate streaming equivalent, such as SSE or a WebSocket).
- HTTP-only adapters are allowed when the browser needs a representation the
  socket cannot carry efficiently: static assets, an SSE stream, image MIME
  detection or a whole-file download response.
- Local CLI utilities (`config get`, `config validate`, `run`, `daemon`) act on
  the local file or process rather than on the daemon, so they have no HTTP
  route. `config show` mirrors `GET /api/settings`.

## Feature matrix

### Host, lifecycle and settings

| Operation | Socket | HTTP | CLI |
| --- | --- | --- | --- |
| Host snapshot | `HostInfo` | `GET /api/host-info` | `host-info` |
| Host stream | `HostInfo` in a loop | `GET /api/host-info/watch` (SSE) | `host-info --watch` |
| Restart | `Restart` | `POST /api/restart` | `restart` |
| Reboot | `SystemReboot` | `POST /api/reboot` | `reboot --delay` |
| Effective settings | — (daemon config) | `GET /api/settings` | `config show` |
| Change one setting | — (config file) | `PATCH /api/settings` | `config set` |
| Raw config file | — | — | `config get` |
| Validate config | — | — | `config validate` |

### Files

| Operation | Socket | HTTP | CLI |
| --- | --- | --- | --- |
| List directory | `FilesList` | `GET /api/files` | `files list` |
| Read UTF-8 text | `FilesRead` | `GET /api/files/read` | `files read` |
| Read arbitrary bytes | `FilesDownload` | `GET /api/files/download` | `files download` |
| Image preview | `FilesDownload` | `GET /api/files/image` | `files download` |
| Write UTF-8 text | `FilesWrite` | `POST /api/files/write` | `files write` |
| Write arbitrary bytes | `FilesUpload` | `POST /api/files/upload` | `files upload` |
| Delete | `FilesDelete` | `DELETE /api/files` | `files delete` |
| Trash | `FilesTrash` | `POST /api/files/trash` | `files trash` |
| Trash list | `FilesTrashList` | `GET /api/files/trash/list` | `files trash-bin list` |
| Trash restore | `FilesTrashRestore` | `POST /api/files/trash/restore` | `files trash-bin restore` |
| Trash empty | `FilesTrashEmpty` | `POST /api/files/trash/empty` | `files trash-bin empty` |
| Create archive | `FilesArchive` | `POST /api/files/archive` | `files archive` |
| List archive | `FilesArchiveList` | `GET /api/files/archive/list` | `files unarchive --list` |
| Extract archive | `FilesArchiveExtract` | `POST /api/files/archive/extract` | `files unarchive` |

`files download` loops `FilesDownload` in 1 MiB chunks and writes the decoded
bytes verbatim to stdout. The HTTP `image` route is the browser preview wrapper:
it validates the image type and adds a MIME header. Both read through the same
`FileService` guards, including `files.max_upload_bytes`.

### Network and firewall

| Operation | Socket | HTTP | CLI |
| --- | --- | --- | --- |
| Interface snapshot | `Network` | `GET /api/network` | `network list` |
| Interface stream | `Network` in a loop | `GET /api/network/watch` (SSE) | `network list --watch` |
| Up/down | `NetworkSetState` | `POST /api/network/state` | `network up/down` |
| Create | `NetworkCreate` | `POST /api/network/create` | `network create` |
| Edit | `NetworkEdit` | `POST /api/network/edit` | `network edit` |
| Rename | `NetworkRename` | `POST /api/network/rename` | `network rename` |
| Delete | `NetworkDelete` | `POST /api/network/delete` | `network delete` |
| Firewall snapshot | `FirewallStatus` | `GET /api/firewall` | `firewall status` |
| Add allow/deny | `FirewallAdd` | `POST /api/firewall/rule` | `firewall allow/deny` |
| Edit rule | `FirewallUpdate` | `POST /api/firewall/update` | `firewall edit` |
| Delete rule | `FirewallDelete` | `POST /api/firewall/delete` | `firewall delete` |
| Flush chain | `FirewallFlush` | `POST /api/firewall/flush` | `firewall flush` |
| Chain policy | `FirewallSetPolicy` | `POST /api/firewall/policy` | `firewall policy` |

### Storage and accounts

| Operation | Socket | HTTP | CLI |
| --- | --- | --- | --- |
| Storage snapshot | `Volumes` | `GET /api/volumes` | `storage list` |
| Mount | `VolumesMount` | `POST /api/volumes/mount` | `storage mount` |
| Unmount | `VolumesUnmount` | `POST /api/volumes/unmount` | `storage unmount` |
| Create partition | `VolumePartitionCreate` | `POST /api/volumes/partition/create` | `storage partition create` |
| Resize partition | `VolumePartitionResize` | `POST /api/volumes/partition/resize` | `storage partition resize` |
| Delete partition | `VolumePartitionDelete` | `POST /api/volumes/partition/delete` | `storage partition delete` |
| Format | `VolumeFormat` | `POST /api/volumes/format` | `storage format` |
| Account snapshot | `Accounts` | `GET /api/accounts` | `account list` |
| Create user | `AccountCreate` | `POST /api/accounts/users` | `account create` |
| Delete user | `AccountDelete` | `POST /api/accounts/users/delete` | `account delete` |
| Set password | `AccountSetPassword` | `POST /api/accounts/users/password` | `account password` |
| Create group | `AccountGroupCreate` | `POST /api/accounts/groups` | `account group create` |
| Delete group | `AccountGroupDelete` | `POST /api/accounts/groups/delete` | `account group delete` |

### Processes and terminal

| Operation | Socket | HTTP | CLI |
| --- | --- | --- | --- |
| Process page | `Processes` | `GET /api/processes` | `process list` |
| Process stream | `Processes` in a loop | `GET /api/processes/watch` (SSE) | `process list --watch` |
| Signal | `ProcessSignal` | `POST /api/processes/signal` | `process signal` |
| Spawn | `ProcessSpawn` | `POST /api/processes/spawn` | `process spawn` |
| Terminal | `TerminalOpen/Read/Write/Resize/Close` | `GET /api/terminal` (WebSocket) | `terminal` |

## HTTP-only routes

These are transport functions, not host operations that need a CLI twin:

| Route | Why it has no CLI |
| --- | --- |
| `GET /healthz` | Process liveness probe for containers and load balancers |
| `GET /` and `GET /{path}` | Embedded web shell and static assets |
| `GET /api/auth/state` | Browser login/registration state |
| `POST /api/auth/register`, `login`, `refresh`, `logout` | Browser session lifecycle; the CLI authenticates with Unix-socket peer credentials |
| `POST /api/auth/password` | Changes the panel password and revokes other web sessions |
| `GET /api/auth/sessions`, `POST /api/auth/sessions/revoke` | Web-session management for the settings page |
| `GET /api/files/image` | Browser preview with MIME detection and CSP; `files download` returns the same bytes |
| SSE `*/watch` routes | Streaming representation of a snapshot the CLI already polls |

## Keeping parity

When a feature slice adds a wire request:

1. Add the dispatch arm and the operation name in `dispatch.rs`.
2. Add the HTTP route and handler.
3. Add the CLI branch that builds the same request.
4. Add a test through the router or socket, and one that asserts the CLI argv or
   request.
5. Add the row to this matrix. If the route is transport-only, add it to the
   exception table with the reason.

The audit was run on 2026-10-28 before Phase 7. It found one missing feature
pair, `files download`, and one missing read view, `config show`; both are in
place now.
