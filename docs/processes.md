# Processes

The daemon can inspect the process table of the host it runs on and, when
explicitly enabled, deliver an allowlisted signal to a process.

Process monitoring reads `/proc` directly. It works without authentication, like
the rest of the panel, so keep the HTTP port on a trusted network or behind a
proxy that authenticates users. Command lines can contain secrets.

## Configuration

Process control and spawning are **off by default**:

```toml
[process]
control = false
spawn = false
spawn_allow = []
```

Set `control` to `true` to allow signal requests. Set `spawn` to `true` and list
absolute program paths in `spawn_allow` to allow starting processes:

```toml
[process]
control = true
spawn = true
spawn_allow = ["/bin/ls", "/usr/bin/uptime"]
```

Changing the file requires a daemon restart. `PATCH /api/config` updates the file
and the API process, but a running daemon keeps the policy it read at startup.

### Spawning

`spawn` is a second, independent switch. Signals can be enabled without
spawning and the reverse. An empty `spawn_allow` denies every request even when
`spawn = true`: the list is the authority, not the switch, so an operator who
forgets the list gets a refusal rather than an open panel.

The daemon validates each request:

- the program must be an absolute path;
- the path is canonicalised, so a symlink that points outside the allowlist is
  refused rather than resolved;
- the canonical path must match an entry in `spawn_allow` exactly;
- a missing working directory, or one that is not a directory, is refused;
- the program inherits the daemon's environment, with stdin closed and output
  discarded — it is a fire-and-forget start, not a job queue.

The daemon does not supervise what it starts. The child calls `setsid()` before
`exec`, so it leads its own session and process group and keeps running if the
daemon stops — including when the whole group is signalled or a service manager
stops the daemon's cgroup.

Debug builds (`cargo run`) bootstrap a missing config with `control = true` so
signals can be tested locally. Release builds still generate `false`, and an
existing file is never rewritten.

Even with control enabled, the daemon refuses to:

- signal pid 1 or its own pid;
- signal a process owned by a different user;
- deliver a signal outside the allowlist.

The allowlist is `term`, `kill`, `hup`, `int`, `quit`, `stop` and `cont`.

## CLI

```bash
anth-panel process list
anth-panel process ls                  # alias
anth-panel process list --watch 2s
anth-panel process signal 1234 term
anth-panel process signal 1234 kill --yes
```

`list` prints PID, PPID, USER, CPU, MEM, STATE and COMMAND. With control
disabled, `signal` returns the daemon error and exits `1`. Without `--yes`,
`signal` asks for confirmation on a TTY and refuses to run non-interactively.

## Data model

Each snapshot contains:

| Field | Meaning |
| --- | --- |
| `pid`, `ppid` | Process and parent process ids |
| `uid`, `user` | Real uid and its `/etc/passwd` name |
| `name` | `/proc/<pid>/comm` |
| `command` | `/proc/<pid>/cmdline`, NUL-separated arguments joined with spaces |
| `state` | One-letter `/proc` state (`R`, `S`, `D`, `Z`, `T`, ...) |
| `cpu_ticks` | `utime + stime` in clock ticks |
| `rss_kib` | Resident set size from `/proc/<pid>/status` |
| `threads` | Number of threads |
| `start_ticks` | Process start time in clock ticks since boot |

Snapshots also carry `clock_ticks`, `timestamp_ms`, `control_enabled`, `total`,
`truncated` and an optional `warning`.

CPU percent is **computed by the client** from two snapshots:

```text
cpu% = (delta_ticks / clock_ticks / delta_seconds) * 100
```

`pid` alone is not a stable identity: use `pid` + `start_ticks` so a recycled pid
does not inherit the old process's CPU counters.

## Truncation

One protocol line is capped at 1 MiB. A snapshot returns at most 2000
processes, truncates each command line to 256 characters, and then checks the
serialized size of the whole snapshot. If it is still too large, commands are
shortened further (down to 8 characters) before the least busy rows are dropped.
Processes are sorted by accumulated CPU, so the busiest ones stay visible.

Dropping rows sets `warning` to a human-readable notice, and `truncated` is
`true`. Shortening command lines also sets `truncated` but intentionally produces
no warning: it is an implementation detail of fitting the protocol frame. The
CLI prints the warning below the table; the web component shows it above the
table.

## HTTP and browser

The endpoints are documented in [api-endpoints.md](api-endpoints.md):

- `GET /api/processes`
- `GET /api/processes/watch?interval=2s`
- `POST /api/processes/{pid}/signal`

The browser components live in `frontend/src/components/`:

- `<anth-process-table>` owns sort state and switches between two renderers:
  `anth-process-flat-table` for the compact flat table and `anth-process-tree`
  for the free-form tree with CSS hierarchy guides and expand/collapse chevrons.
  Both show PID, COMMAND, THREADS, USER, STATE, MEM and CPU, in that order; PPID
  is used for nesting but is not displayed. The tree does not sort by PID or
  COMMAND. Its command stays on one line; the selection preview keeps the full
  command, left-aligned and vertically centered.
- `<anth-process-manager>` fetches snapshots, follows the SSE stream, computes
  CPU percentages, sends signals and starts allowlisted programs through an
  injected `ProcessClient`. It owns the flat/tree toggle and filtering. The
  spawn button is disabled with a reason when the snapshot reports
  `spawn_enabled = false` or an empty `spawn_allow`, so the form never offers an
  action the daemon would refuse.

`frontend/src/components/anth-process-manager.test.html` is the dev-only
harness. The same behaviour is covered by the Vitest tests under
`frontend/src/components/` and `frontend/src/lib/processes/`.
