# Files

The filesystem explorer exposes one host directory to the socket CLI and the web
UI. It is read-only until writes are explicitly enabled.

## Configuration

```toml
[files]
root = "/"
write = false
max_entries = 5000
max_preview_bytes = 1048576
max_upload_bytes = 67108864
```

| Key | Default | Meaning |
| --- | --- | --- |
| `root` | `/` | Host directory exposed as virtual `/`. Must be absolute and exist when the daemon starts. |
| `write` | `false` | Enables `mkdir`, `rename`, `delete` and `upload`. |
| `max_entries` | `5000` | Maximum entries returned by one listing. |
| `max_preview_bytes` | `1 MiB` | Inline preview limit used by the content endpoint. |
| `max_upload_bytes` | `64 MiB` | Maximum size of one upload. |

All limits must be greater than zero. A bad `root` or a zero limit stops the
daemon with a configuration error.

## Virtual paths

Paths on the wire are virtual:

| Wire path | Host path when `root = /srv` |
| --- | --- |
| `/` | `/srv` |
| `/logs` | `/srv/logs` |
| `/logs/app.txt` | `/srv/logs/app.txt` |

The daemon:

- collapses empty and `.` components;
- rejects `..` components outright;
- canonicalizes existing targets and rejects anything that resolves outside
  `root`;
- lists symlinks without following them, including their raw `target`;
- reads through a symlink only when its canonical target stays inside `root`;
- deletes and renames a symlink itself, never its target.

`root = "/"` means there is no containment boundary beyond the host itself. The
default is convenient, not a security boundary.

## Read path

File content is transferred in chunks so a frame stays below the 1 MiB socket
line limit:

- `FileRead` is stateless and takes `offset` and `len`.
- `len` is clamped to 512 KiB; clients normally request 256 KiB.
- `FileChunk` returns `size`, `eof` and base64 `bytes`.
- The CLI and HTTP API advance the offset until `eof`.

The content endpoint stops at `max_preview_bytes` and reports
`x-anth-truncated: true`. The download endpoint keeps reading until `eof`, so
large files are streamed instead of buffered in a single frame.

## Write path

Writes require `[files] write = true`. When it is off, the daemon returns
`FileError` with `kind = "disabled"`, the API returns `403`, and the web
component renders a read-only badge.

Supported operations:

- create directory (`recursive` optional);
- rename without overwriting;
- delete a file, symlink or directory (`recursive` required for a non-empty
  directory);
- upload a file.

Uploads are atomic:

1. `FileWriteStart` creates a hidden temporary file in the destination
   directory.
2. `FileWriteChunk` appends chunks, enforcing `max_upload_bytes`.
3. `FileWriteFinish` flushes, syncs and renames the temporary file.
4. `FileWriteAbort`, a failed chunk or a dropped connection removes the
   temporary file.

The virtual root cannot be renamed, deleted or used as an upload destination.

## CLI

```bash
anth-panel file list /
anth-panel file read /etc/hostname
anth-panel file read /var/log/syslog --output ./syslog
anth-panel file mkdir /var/tmp/example
anth-panel file upload ./report.pdf /var/tmp/report.pdf
anth-panel file rename /var/tmp/a /var/tmp/b
anth-panel file delete /var/tmp/example --recursive --yes
```

The `file` command and its subcommands have no aliases.

## HTTP API

| Method | Route |
| --- | --- |
| `GET` | `/api/files?path=/` |
| `GET` | `/api/files/content?path=/file` |
| `GET` | `/api/files/download?path=/file` |
| `POST` | `/api/files/mkdir` |
| `POST` | `/api/files/rename` |
| `POST` | `/api/files/delete` |
| `POST` | `/api/files/upload?path=/file` |

See [api-endpoints.md](api-endpoints.md#files) for bodies, headers and status
codes, and [socket-protocol.md](socket-protocol.md#filesystem-requests) for the
wire format.

## Web component

`frontend/src/components/anth-file-explorer.ts` implements breadcrumbs, sorting,
hidden-file filtering, previews, uploads and the write controls. It depends on
the injected `FileClient`; `frontend/src/lib/files/file-client.ts` implements the
HTTP version.

The dev harness is `frontend/anth-file.test.html`. It is not part of the Vite
production build. Run it with:

```bash
cd backend && cargo run -p anth-panel -- run
cd frontend && npm run dev
# open http://localhost:5173/anth-file.test.html
```

Unit tests cover the client, display helpers and component under
`frontend/src/lib/files/` and `frontend/src/components/`.

## Security

Authentication is still unimplemented, as with the terminal and process routes.
`root` limits which paths the explorer can reach, but anyone who can reach the
HTTP API can browse that tree. Keep `write = false` unless the panel is on a
trusted network, and prefer a narrow `root` such as `/srv` or a user's home over
`/`.
