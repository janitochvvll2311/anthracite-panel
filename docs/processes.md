# Processes

The daemon can inspect the process table of the host it runs on and, when
explicitly enabled, deliver an allowlisted signal to a process.

Process monitoring reads `/proc` directly. It works without authentication, like
the rest of the panel, so keep the HTTP port on a trusted network or behind a
proxy that authenticates users. Command lines can contain secrets.

## Configuration

Process control is **off by default**:

```toml
[process]
control = false
```

Set it to `true` to allow signal requests:

```toml
[process]
control = true
```

Changing the file requires a daemon restart.

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

One protocol line is capped at 1 MiB, so a snapshot returns at most 2000
processes and truncates each command line to 256 characters. Processes are sorted
by accumulated CPU before truncation, so the busiest ones stay visible.

When rows or command lines were shortened, `warning` is a human-readable notice
and `truncated` is `true`. The CLI prints the warning below the table; the web
component shows it above the table.

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
  command.
- `<anth-process-monitor>` fetches snapshots, follows the SSE stream, computes
  CPU percentages and sends signals through an injected `ProcessClient`. It
  owns the flat/tree toggle and filtering.

`frontend/anth-process.test.html` is the dev-only harness.
