# Demo mode

The hosted demo is in progress. The network, firewall, files, process, terminal,
storage and account backends now live in `anth-host`, and each has a
deterministic virtual implementation for the demo build.

## Development switch

```bash
ANTH_BACKEND=real ./scripts/dev.sh start   # current daemon via sudo
ANTH_BACKEND=demo ./scripts/dev.sh start   # no sudo, simulated host
```

`ANTH_BACKEND=demo` uses a separate run directory `/tmp/anthracite-panel/demo`
so it cannot mix state with a real development run. It starts the daemon as the
calling user, without `sudo`, and sets `ANTH_BACKEND=demo` for the process.

The dev helper prints a warning that every host feature is simulated. In this
mode no privileged command runs and no PTY is opened; the daemon still reads the
host for `host-info`, because that view is informational.

## Current demo data

- `lo`, `eth0` and `wlan0`, with IPv4/IPv6 addresses and one default route.
- Network up/down/create/edit/rename/delete update the in-memory snapshot.
- Loopback, physical interfaces and the last default-route interface are
  protected, matching the real guards.
- `inet anthracite` starts with both chains and an `allow` policy.
- Firewall add/update/delete/flush/policy update the in-memory table.
- `[network] control/manage` and `[firewall] control` are still enforced.
- Files use a seeded sandbox at `<state_dir>/demo/files`; writes, uploads,
  archives and trash stay there.
- The process table is synthetic; signals and spawns mutate it without touching
  the kernel, and the configured control/spawn switches and allowlist apply.
- Storage starts with two demo disks and their partitions. Mount/unmount,
  partition create/resize/delete and format update an in-memory table;
  `/dev/sdb` is the allowed demo disk when `disk_allow` is empty.
- Accounts start with `root`, `demo` and `www-data` plus their groups. Create and
  delete users/groups and password changes update an in-memory table; the
  control, password, system and privileged-group switches still apply.
- The terminal is a virtual shell. It echoes input and answers `help`, `whoami`,
  `pwd`, `ls`, `uname`, `echo`, `date`, `clear` and `exit`; no PTY or signal is
  involved.
- Snapshots use the same wire types, so the existing UI and CLI work unchanged.

The state is shared by all clients and lost on restart. Per-session state,
feature toggles, the demo banner, reset endpoint and the separate hosted image
are the next phases in `/root/.opencode/plan/anthracite-demo.md`.

## Host boundary

`network`, `firewall`, `files`, `process`, `terminal`, `volumes` and `accounts`
live in `anth-host`:

```text
anth-host/src/network
anth-host/src/firewall
anth-host/src/files
anth-host/src/process
anth-host/src/terminal
anth-host/src/volumes
anth-host/src/accounts
anth-host/src/backend.rs   traits used by anth-panel
anth-host/src/demo         virtual implementations
```

`anth-panel` keeps policy, guards, dispatch, HTTP/CLI rendering and UI. It talks
to `<Feature>Service`, so the same request path serves the real and demo
implementations.
