# Client parity: Rust and TypeScript

`anth-wire` and `anth-client` exist twice: a Rust crate and a TypeScript
package. They expose one client with the same operations, the same DTO returns
and the same error fields. Only the spelling follows the language: Rust uses
`snake_case` and `Result<T, WireError>`; TypeScript uses `camelCase` and
exceptions.

## Layers

| Layer | Rust | TypeScript |
| --- | --- | --- |
| Contract | `anth-wire` DTOs, `WireError`, terminal messages | `anth-wire` interfaces, DTOs, `WireError` body |
| Implementation | `anth-client::Client` | `anth-client::AnthClient` |
| Transport | `Endpoint::{Unix, Http}` + bearer | `HttpTransport { baseUrl, token, headers, fetch }` |
| Errors | `WireError { kind, code, message }` | `WireError extends Error { kind, code, message, status }` |
| Snapshots | `EventStream<T>::next()` | `EventStream<T>::next()` |
| Terminal | `TerminalSession::{input, resize, next, close}` | `TerminalSession::{input, resize, next, close}` |

## Construction

```rust
let local = anth_client::Client::unix("/run/anth/anth.sock");
let remote = anth_client::Client::http("http://127.0.0.1:8080")
    .with_bearer("token");
```

```ts
const local = new AnthClient()
const remote = new AnthClient({
  baseUrl: 'http://127.0.0.1:8080',
  token: 'token',
})
```

A TypeScript client also accepts a bare `fetch` (or a prebuilt `HttpTransport`)
when embedding or testing. Rust takes an `Endpoint`; TLS is terminated by the
reverse proxy in both cases.

## Method map

The conversion is mechanical: `snake_case` → `camelCase`. Arguments and returns
are the same DTOs.

| Rust | TypeScript |
| --- | --- |
| `hello()` | `hello()` |
| `host_info()` | `hostInfo()` |
| `watch_host_info(ms)` | `watchHostInfo(ms)` |
| `network()` | `network()` |
| `watch_network(ms)` | `watchNetwork(ms)` |
| `network_set_state(name, up)` | `networkSetState(name, up)` |
| `network_create(&NetworkCreate)` | `networkCreate(request)` |
| `network_edit(&NetworkEdit)` | `networkEdit(request)` |
| `network_rename(name, new)` | `networkRename(name, newName)` |
| `network_delete(name)` | `networkDelete(name)` |
| `firewall()` | `firewall()` |
| `firewall_add(...)` | `firewallAdd(...)` |
| `firewall_update(...)` | `firewallUpdate(...)` |
| `firewall_delete(handle)` | `firewallDelete(handle)` |
| `firewall_flush(chain)` | `firewallFlush(chain)` |
| `firewall_policy(chain, policy)` | `firewallPolicy(chain, policy)` |
| `volumes()` | `volumes()` |
| `volume_mount(&VolumeMountRequest)` | `volumeMount(request)` |
| `volume_unmount(&VolumeUnmountRequest)` | `volumeUnmount(request)` |
| `volume_partition_create(...)` | `volumePartitionCreate(request)` |
| `volume_partition_resize(...)` | `volumePartitionResize(request)` |
| `volume_partition_delete(...)` | `volumePartitionDelete(request)` |
| `volume_format(...)` | `volumeFormat(request)` |
| `accounts()` | `accounts()` |
| `account_create(&AccountCreate)` | `accountCreate(request)` |
| `account_delete(&AccountDelete)` | `accountDelete(request)` |
| `account_password(&AccountSetPassword)` | `accountPassword(request)` |
| `account_group_create(...)` | `accountGroupCreate(request)` |
| `account_group_delete(...)` | `accountGroupDelete(request)` |
| `processes(limit, cursor)` | `processes(limit, cursor)` |
| `watch_processes(ms, limit)` | `watchProcesses(ms, limit)` |
| `process_signal(pid, ticks, sig, group)` | `processSignal(pid, ticks, signal, group)` |
| `process_spawn(program, args, cwd)` | `processSpawn(program, args, cwd)` |
| `files_list(path, &PageRequest)` | `filesList(path, request)` |
| `files_read(path, offset, max)` | `filesRead(path, offset, maxBytes)` |
| `files_download(path)` | `filesDownload(path)` |
| `files_delete(path)` | `filesDelete(path)` |
| `files_write(path, content, create)` | `filesWrite(path, content, create)` |
| `files_upload(path, base64, create)` | `filesUpload(path, base64, create)` |
| `files_trash(path)` | `filesTrash(path)` |
| `trash_list()` | `trashList()` |
| `trash_restore(id, overwrite)` | `trashRestore(id, overwrite)` |
| `trash_empty()` | `trashEmpty()` |
| `archive_create(path, dest, format)` | `archiveCreate(path, dest, format)` |
| `archive_list(path)` | `archiveList(path)` |
| `archive_extract(path, into)` | `archiveExtract(path, into)` |
| `restart()` | `restart()` |
| `reboot(delay_s)` | `reboot(delaySeconds)` |
| `terminal(&TerminalSize)` | `terminal(size)` |
| `request_raw(method, path, body)` | `requestRaw(method, path, body)` |

Mutations return the same DTO: `Change`, `FileDeleted`, `FileTrashed`,
`FileRestored`, `TrashEmptied`, `ArchiveCreated`, `ArchiveExtracted`,
`ProcessSignalled`, `Restarting` or `Rebooting`. No client flattens a body to a
bare string.

## Streams

```rust
let mut stream = client.watch_network(2_000).await?;
while let Some(snapshot) = stream.next().await? {
    render(&snapshot);
}
```

```ts
const stream = client.watchNetwork(2_000)
for await (const snapshot of stream) {
    render(snapshot)
}
```

Both take milliseconds and build the same `?interval=<ms>ms` query. `next()`
returns `null`/`None` at the end; both expose `close()`.

The TypeScript per-feature classes keep a `watch(intervalMs, handlers)`
convenience for Lit components. It is a thin adapter over the same stream.

## Terminal

```rust
let mut session = client.terminal(TerminalSize { cols: 120, rows: 40 }).await?;
session.input(b"ls\n").await?;
while let Some(message) = session.next().await? { /* ... */ }
```

```ts
const session = await client.terminal({ cols: 120, rows: 40 })
session.inputText('ls\n')
while (true) {
  const message = await session.next()
  if (!message) break
  // ...
}
```

Both start with `ready`, then `output` (base64), then `exit`; `error` and `pong`
are the other messages. `input`/`resize` are void in TypeScript and
`Result<(), WireError>` in Rust because of the language.

## Intentional differences

- **Browser-only extras.** `auth`, `settings` and `apps` hang off `AnthClient`;
  the Rust client covers the host surface. The CLI authenticates with peer
  credentials and manages apps locally, so it does not need those routes. A
  remote Rust integration can still call them with `request_raw`.
- **Binary download.** Rust returns a `Download` that yields chunks; TypeScript
  returns a `Blob` because `fetch` already buffers it that way.
- **Bulk upload.** Rust takes the base64 string and the caller encodes the
  bytes; TypeScript has the same signature. Neither client invents a multipart
  route the daemon does not serve.
- **WebSocket authorization.** A browser `WebSocket` cannot set an
  `Authorization` header, so a bearer-token deployment must keep the panel on
  the same origin. The Rust client can set the header.
- **Local authentication.** Only the Rust client has an `Endpoint::Unix`; the
  browser talks to the same origin, and the panel injects the cookie.
- **TLS.** Neither client terminates TLS. The daemon serves HTTP on TCP and the
  reverse proxy terminates TLS.

## Rules

When an operation lands:

1. Add the DTO to `anth-wire` (Rust) and the TypeScript DTO/interface.
2. Add the typed method to `anth-client` in both languages.
3. Use the same return DTO on both sides.
4. Add or update the row in the method map.
5. Add one test per language that asserts the URL, the body and the decoded
   return.
