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

A build compiled with `--features demo` forces this mode even when
`ANTH_BACKEND=real` is in the environment. The hosted artifact uses that feature,
so it cannot select the real backends by mistake.

## Sessions

Every browser gets an anonymous `anth_demo` cookie. Its value is generated
server-side and scopes one complete demo machine: network, firewall, files,
processes, storage, accounts and terminal state. Two visitors never see each
other's changes, and a socket CLI keeps its own `daemon` session.

- `[demo] session_ttl_seconds` drops a session after that much idle time; `0`
  disables expiry. The reaper runs every 30 seconds.
- `[demo] max_sessions` evicts the least recently used session instead of
  refusing a new visitor.
- `POST /api/demo/reset` drops the current session and its sandbox; the next
  request seeds a fresh one.
- `GET /api/demo` reports the mode, feature matrix, session timestamps, TTL and
  live-session count.
- `[demo] features` is empty for every feature, or a list such as
  `["network", "files", "terminal"]`. A disabled feature answers
  `demo_feature_disabled`.

The shell shows a **Demo** banner, a chip on each simulated section and a
**Reset demo** button.

## Current demo data

- `lo`, `eth0` and `wlan0`, with IPv4/IPv6 addresses and one default route.
- Network up/down/create/edit/rename/delete update the in-memory snapshot.
- Loopback, physical interfaces and the last default-route interface are
  protected, matching the real guards.
- `inet anthracite` starts with both chains and an `allow` policy.
- Firewall add/update/delete/flush/policy update the in-memory table.
- `[network] control/manage` and `[firewall] control` are still enforced.
- Files use a per-session sandbox at
  `<state_dir>/demo/sessions/<hashed owner>/files`; writes, uploads, archives
  and trash stay there.
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

Each session is independent and lost on restart. Plugins are still the global
plugin service: the fixed demo catalogue is the next phase in
`/root/.opencode/plan/anthracite-demo.md`.

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

## Hosted image

```bash
./scripts/package-demo.sh 0.3.0a
cd dist
tar -xzf anthracite-panel-demo-0.3.0a-x86_64-unknown-linux-musl.tar.gz
cd anthracite-panel-demo-0.3.0a-x86_64-unknown-linux-musl
docker build --build-arg VERSION=0.3.0a -t anthracite-panel-demo:0.3.0a .
docker run --rm --read-only --tmpfs /var/lib/anth:size=256m \
  -p 127.0.0.1:8080:8080 anthracite-panel-demo:0.3.0a
```

The demo image runs as UID 10001, needs no Linux capabilities, keeps no host
volumes and stores everything in the tmpfs at `/var/lib/anth`. It binds
`0.0.0.0:8080` inside the container; publish it on loopback and let the reverse
proxy terminate TLS and apply public rate limits. The image has no TLS
configuration on purpose.

The packaged config enables every demo policy, disables real authentication and
disables plugins. Edit `etc/anth/config.toml` before building the image to change
the TTL, session cap or feature matrix.
