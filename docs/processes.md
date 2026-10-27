# Processes

The daemon reads the process table from `/proc` and, when explicitly enabled,
delivers an allowlisted signal or starts an allowlisted program.

Reading works with the normal panel session; signals and spawns are two
independent switches that default to off. Command lines can contain secrets, so
keep the HTTP port on a trusted network or behind an authenticating proxy.

## Configuration

```toml
[process]
control = false
spawn = false
spawn_allow = []
```

- `control` gates signals. With it off, `process list` still works and the UI
  hides the action bar.
- `spawn` gates starting programs, and `spawn_allow` lists the absolute paths
  that may be started. An empty list denies everything even with `spawn = true`:
  the list is the authority, not the switch.
- A change requires a daemon restart; the running daemon keeps the policy it
  read at startup.

Debug builds (`cargo run`) bootstrap a missing config with both switches on and
`/bin/echo`, `/bin/sleep` and `/usr/bin/uptime` allowed, so the UI can be
exercised locally. Release builds generate the defaults above and never rewrite
an existing file.

## Data model

Each row carries:

| Field | Meaning |
| --- | --- |
| `pid`, `ppid` | Process and parent process ids |
| `uid`, `user` | Real uid and its `/etc/passwd` name |
| `name` | `/proc/<pid>/comm` |
| `command` | `/proc/<pid>/cmdline` joined with spaces, or `[name]` for kernel threads |
| `state` | One-letter `/proc` state (`R`, `S`, `D`, `Z`, `T`, ...) |
| `cpu_ticks` | `utime + stime` in clock ticks |
| `rss_kib` | Resident set size |
| `threads` | Thread count |
| `start_ticks` | Start time in clock ticks since boot |

`pid` alone is not an identity: the kernel recycles it. **`pid + start_ticks`**
is the stable identity, and every mutation carries the `start_ticks` the caller
saw. A mismatch is refused with `conflict` / `process_changed`, never applied to
whatever inherited the number.

A snapshot also carries `clock_ticks` (kernel ticks per second),
`timestamp_ms`, `control_enabled`, `spawn_enabled` and `spawn_allow`.

CPU percent is computed by the client from two consecutive snapshots:

```text
cpu% = (delta_ticks / clock_ticks / delta_seconds) * 100
```

A value above 100 means more than one core. The web table keeps the previous
sample per `pid:start_ticks`; the first frame shows `—`.

## Pagination

The table is sorted by accumulated CPU, busiest first, and then by pid. It is
never shortened to fit a frame:

- `GET /api/processes?limit=100&cursor=...` (socket: `processes`) returns
  `items`, `next` and `total`.
- `limit` is 1–1000 (default 100). `next` is an opaque offset into that sorted
  snapshot; `next = null` is the only end signal.
- The web table asks for 1000 rows in one live page and shows `N / M
  processes`; when the host has more than a page, the count's tooltip says that
  only the first page is loaded.

A cursor describes the snapshot it came from. The table changes between ticks,
so a cursor is for walking one listing, not for resuming a live feed.

## Signals

The allowed signals are `term`, `kill`, `hup`, `int`, `quit`, `stop` and `cont`.
`SIG` prefixes and any case are accepted, so `SIGTERM` and `sigterm` are the same
request.

Even with `control = true`, the daemon refuses to:

- signal pid 1 or its own pid;
- signal a process owned by a different user;
- signal a process whose `start_ticks` differs from the request;
- signal the process group when `pid` does not lead its group.

`group: true` sends the signal to the process group led by `pid`
(`kill(-pid, ...)`), which reaches children that stayed in the group. It is an
explicit choice: a service leader can have many children, so the plain request
signals only the process.

## Spawning

A spawn request names an absolute program, optional arguments and an optional
absolute working directory. The daemon validates:

- the program resolves to a regular executable file;
- its canonical target matches an entry in `spawn_allow` **and** the file name
  matches that entry;
- the working directory exists and is absolute;
- at most 64 arguments, each at most 4 KiB.

The file-name check matters on busybox systems, where `true`, `sleep` and `sh`
are symlinks to one binary: comparing only canonical paths would let an operator
who listed `/bin/true` start `/bin/sh`. The merged-usr alias still works, so
`/bin/echo` and `/usr/bin/echo` are the same program.

The program runs with `stdin`, `stdout` and `stderr` set to `/dev/null`, calls
`setsid()` before `exec` so it leads its own session, and is reaped by a thread.
It is a fire-and-forget start: no output, no exit code, no supervision and no
job queue. Arguments are passed as an argv vector; nothing goes through a shell
and nothing is expanded.

## HTTP

All routes require a live panel session unless auth is disabled.

### `GET /api/processes?limit=5&cursor=...`

```json
{
  "items": [
    {
      "pid": 42,
      "ppid": 1,
      "uid": 0,
      "user": "root",
      "name": "sleep",
      "command": "sleep 30",
      "state": "S",
      "cpu_ticks": 12,
      "rss_kib": 1024,
      "threads": 1,
      "start_ticks": 98765
    }
  ],
  "next": null,
  "total": 793,
  "clock_ticks": 100,
  "timestamp_ms": 1790000000000,
  "control_enabled": true,
  "spawn_enabled": true,
  "spawn_allow": ["/bin/sleep"]
}
```

### `GET /api/processes/watch?interval=2s&limit=1000`

Server-Sent Events, the same shape as `host-info/watch`:

- `event: processes`
- `id`: `timestamp_ms`
- `data`: one snapshot (the first page)
- `retry: 3000`
- keep-alive comments every 15 seconds.

### `POST /api/processes/signal`

```json
{
  "pid": 42,
  "start_ticks": 98765,
  "signal": "term",
  "group": false
}
```

`200 {"pid":42,"signal":"term"}`. `403 process_control_disabled`,
`403 protected_process`, `403 process_not_owned`, `409 process_changed`,
`400 not_group_leader`.

### `POST /api/processes/spawn`

```json
{ "program": "/bin/sleep", "args": ["30"], "cwd": "/tmp" }
```

`200 {"pid":99,"program":"/bin/sleep","args":["30"]}`. `403
process_spawn_disabled`, `403 program_not_allowed`, `403
program_not_executable`, `404 not_found`, `400 program_not_absolute`, `400
cwd_not_absolute`, `404 cwd_not_found`.

## CLI

```bash
anth-panel process list
anth-panel process ls --limit 20
anth-panel process list --watch 2s
anth-panel process signal 1234 term
anth-panel process signal 1234 kill --yes
anth-panel process signal 1234 term --group
anth-panel process spawn /bin/sleep 30
anth-panel process spawn --cwd /tmp /bin/uptime
```

`list` prints PID, PPID, USER, STATE, TICKS, MEM, THR and COMMAND. Ticks are
accumulated CPU clock ticks; use `--watch` to follow the table. `signal` looks
up the process to read its `start_ticks`, asks for confirmation on a TTY and
refuses without `--yes` when stdin is not a terminal. `spawn` prints the pid of
the started program; because every remaining word is passed as an argument, put
`--cwd` before the program.

## Web UI

**Processes** sits next to **Terminal** in the sidebar. It follows v0.1's
process manager:

- Two views share one sorted row set. **Flat** has PID, COMMAND, THREADS, USER,
  STATE, MEM and CPU as columns. **Tree** nests by `ppid`; each node carries its
  pid and command immediately after its guides, so they follow the indentation
  instead of sharing a fixed column with the rest of the table. Branches have a
  chevron, collapse independently and stay collapsed across refreshes. Header
  clicks sort by any metric; PID and COMMAND are not sortable in the tree,
  where the hierarchy owns the order.
- The feed is SSE: the refresh selector offers 1, 2, 5, 10 and 30 seconds, and
  Pause/Resume stops and restarts the stream. A manual Refresh fetches one page
  without restarting it.
- CPU percentages are computed from two consecutive snapshots, keyed by
  `pid:start_ticks`; the first frame shows `—`. States read as words (`running`,
  `sleeping`, `zombie`, ...) with colour for running, stopped and zombie.
- The filter matches pid, user, name, command and state.
- The table is split into two panes: the **identity** pane (tree gutter, pid and
  command) and the **metric** pane (threads, user, state, memory, CPU). Only the
  identity pane scrolls sideways — a long command or a deep tree moves there
  while the metrics stay on screen. Both panes share the vertical scroll.
- Selecting a row highlights it across both panes and fills the footer with its
  identity, command and fresh metrics: state, thread count, memory and CPU.
  The footer reads the selected row from the latest snapshot on every SSE
  frame, so it updates with the table. With `process.control`, it also offers
  the signal list, a **Group** switch and **Send signal** after a confirmation
  dialog; without control it says so and offers nothing.
- **Start process** only appears with `process.spawn`. The form takes a command
  line and an optional working directory, and lists `spawn_allow` as chips that
  replace the program word while keeping the arguments. Arguments follow the
  documented quoting rules — whitespace separates them, quotes group them and
  there is no shell expansion. The daemon validates the allowlist again.

On a phone the two panes become two-line cards: the identity (tree gutter, pid
and command) on the first line, the metrics (user, threads, state, memory and
CPU) below it. The command wraps instead of forcing a sideways scroll, and a
sort selector plus a direction button sit next to the filter. The same fresh
metrics appear in the details footer.
