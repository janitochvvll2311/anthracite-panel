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

Debug builds bootstrap a missing config with `write = true` so the write
controls are testable locally. Release builds still generate `write = false`,
and an existing file is never rewritten.

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

`frontend/src/components/anth-file-manager.ts` implements breadcrumbs, sorting,
hidden-file filtering, previews, the text editor, archives (create, list and
extract), the recycle bin and the write controls. It depends on
the injected `FileClient`; `frontend/src/lib/files/file-client.ts` implements the
HTTP version.

The manager has a bounded height by default, so the file list scrolls inside the
component and the preview stays visible. Override it with the
`--anth-file-height` CSS property when embedding it:

```css
anth-file-manager {
  --anth-file-height: 620px;
}
```

The dev harness is
`frontend/src/components/anth-file-manager.test.html`. It is not part of the
Vite production build. Run it with:

```bash
cd backend && cargo run -p anth-panel -- run
cd frontend && npm run dev
# open http://localhost:5173/src/components/anth-file-manager.test.html
```

Unit tests cover the client, display helpers and component under
`frontend/src/lib/files/` and `frontend/src/components/`.

## Recycle bin

`[files] trash = true` makes `file trash` and `POST /api/files/trash` move an
entry to the [freedesktop.org trash](https://specifications.freedesktop.org/trash-spec/latest/)
instead of deleting it. `file delete` stays unconditional: the panel asks before
it, and a permanent delete has to remain reachable.

Each volume keeps its own trash, so a move never crosses a filesystem and stays
an atomic `rename`:

| Volume | Directory |
| --- | --- |
| The filesystem holding `$HOME` | `$XDG_DATA_HOME/Trash` (default `~/.local/share/Trash`) |
| Any other volume | `<topdir>/.Trash/$uid`, else `<topdir>/.Trash-$uid` |

`<topdir>/.Trash/$uid` is only used when `<topdir>/.Trash` is a real directory,
not a symlink, world-writable and sticky. The home volume is chosen by comparing
the device, not the path: in a container `/` and `/home` are one overlay, and a
`.Trash-UID` at `/` would need root.

Entries are stored under their original name in `files/` and described by
`info/<name>.trashinfo`:

```ini
[Trash Info]
Path=/notes.txt
DeletionDate=2026-10-21T06:06:40
```

`Path` is percent-encoded as the specification requires, so a name with spaces,
`%` or non-ASCII text survives unchanged. A taken name gets a numbered suffix
(`notes.txt`, `notes.txt.1`, ...), as the specification prescribes. The file is
written before the move and removed only after a successful restore, so a crash
leaves either a restorable entry or nothing — never an entry that cannot be
restored.

### Restoring

`file trash-bin restore ID` reads `Path` back and re-validates it: it is
percent-decoded, must normalize without `..`, and is resolved inside
`files.root`. The value comes from whichever application deleted the file, which
may not be this daemon, so it is treated as untrusted input. An existing target
is refused unless `--overwrite` is given, and `--overwrite` never replaces a
directory.

### Retention

`[files] max_trash_bytes` (default 1 GiB, `0` for no ceiling) drops the oldest
entries after a move. Retention is best effort: it runs after the move has
already succeeded, and a failure to prune does not fail the request.

A volume without a writable trash directory is refused rather than deleted
permanently, with a message naming the directory. That is the deliberate
difference from a desktop file manager: silently turning a reversible delete
into an irreversible one is worse than an error.

## Text editing

The file manager opens a text file in a CodeMirror editor with line numbers,
syntax highlighting and word autocompletion. The grammar comes from the file
name; formats without a grammar the panel ships (`shell`, `toml`, plain text)
keep line numbers and bracket matching rather than pretending to highlight.

`file write PATH SOURCE` (or `-` for stdin) and `POST /api/files/write` save the
buffer. The HTTP body is the raw text, with `path` and `expected_modified_ms` as
query parameters: a JSON envelope would put the whole file in one socket line,
and the daemon caps a line at 1 MiB, so a large file could be opened but not
saved. Both paths share one rule:

- **Saving is atomic and streams.** The text travels in chunks through the same
  upload flow, goes to a temporary file in the same directory and is renamed
  over the target, so a crash or a full disk leaves the old file intact instead
  of a truncated one. The replacement keeps the target's mode, so saving a
  `0600` config cannot publish it.
- **A stale save is refused.** The editor sends the mtime it loaded. If the file
  changed since, the daemon answers `409` with a `Conflict` and writes nothing,
  because two people editing the same file must not silently overwrite each
  other. Omitting the precondition is an explicit "overwrite".
- **Saving never creates a file.** A deleted file stays deleted; creating one is
  `upload`.
- **Only text.** A NUL byte in the body is refused, and the size is bounded by
  `[files] max_upload_bytes`.
- **Symlinks are written through.** A path that resolves to a symlink updates
  its target, so editing a linked config file does not detach the link.
- **A truncated preview is read-only.** The editor is disabled when the preview
  stopped short of the file, because saving it would write only that slice back.
- **Delete uses the trash when it is on.** With `[files] trash = true`, the
  panel moves the selection to the freedesktop trash and the prompt says it can
  be restored; otherwise it deletes permanently and says so.

## Archives

`file archive PATH` compresses one entry; `file unarchive PATH` extracts it and
`file unarchive --list PATH` inspects it without writing anything. Archives are
handled in-process: the runtime image has no `7z` or `xz`, and its `tar` is
BusyBox, so delegating would place the security boundary in behaviour this
project cannot pin in a test.

| Format | List | Extract | Create |
| --- | --- | --- | --- |
| `.zip` | yes | yes | yes |
| `.tar` | yes | yes | yes |
| `.tar.gz`, `.tgz` | yes | yes | yes |
| `.tar.xz`, `.txz` | yes | yes | yes |
| `.7z` | yes | yes | no |

Creating `.7z` is refused with an explicit message rather than silently
producing a file that other tools cannot read.

The format comes from the file name and, when that name is unknown or missing,
from the leading bytes (`PK`, `7z`, `\xfd7zXZ`, `\x1f\x8b`, `ustar`).

### Extraction guards

An archive is untrusted input, so extraction refuses rather than repairs:

- **Absolute paths, `..` and Windows drive letters** are rejected, and so is a
  backslash anywhere in a member name. A backslash is a legal character on Unix
  and a separator on Windows, so a file named `..\..\x` would become an escape
  the moment the tree is copied.
- **A symlink an archive later writes through** is stopped by creating every
  intermediate directory and refusing to traverse one that is a symlink. This is
  the classic two-step tar escape: store `link -> /tmp`, then `link/owned.txt`.
  The final component is opened with `O_NOFOLLOW`, so the check and the open are
  a single kernel decision rather than two syscalls a concurrent writer could
  interleave.
- **Zip bombs** are caught while writing, not after. `[files] max_archive_bytes`
  (default 4 GiB) caps the total uncompressed size and each entry, and
  `max_entries` caps how many members one archive may hold. The counter is
  checked per 64 KiB chunk, so an entry that lies about its declared size cannot
  fill the disk first.
- **`.tar.xz` is decoded as a stream**, not inflated into memory first. The
  container is walked block by block and each LZMA2 chunk is pulled through a
  `Read`, so peak memory follows the read buffer rather than the expansion: a
  39 KB archive that expands to 256 MiB costs under a megabyte. The dictionary
  size a block declares is capped at 256 MiB, because the decoder allocates it
  up front. BCJ and delta filters are refused rather than ignored, and the block
  checks (CRC32, CRC64, SHA-256) are skipped rather than verified.
- **A refused member fails the request.** Skipping it would leave a partial tree
  that looks complete.

`docs.zip`, whose members are `docs/a.txt`, is unwrapped when the destination is
already named `docs`; extracting into an unrelated directory keeps the archive's
layout. Only an archive where *every* member shares one leading directory is
eligible, so a mixed archive is never half-relocated.

## Security

Authentication is still unimplemented, as with the terminal and process routes.
`root` limits which paths the explorer can reach, but anyone who can reach the
HTTP API can browse that tree. Keep `write = false` unless the panel is on a
trusted network, and prefer a narrow `root` such as `/srv` or a user's home over
`/`.

Inline previews never execute active content on the panel origin: HTML, SVG and
other non-image files are served as `text/plain` with
`x-content-type-options: nosniff`; only safe raster image types keep an image
MIME. Downloads keep their guessed MIME but are served as attachments.
