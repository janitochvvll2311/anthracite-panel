# Demo mode

The hosted demo is in progress. The first slices move the network, firewall,
files and process backends into `anth-host` and add deterministic virtual
implementations. Storage, accounts and the terminal still use the real host
until their backends move.

## Development switch

```bash
ANTH_BACKEND=real ./scripts/dev.sh start   # current daemon via sudo
ANTH_BACKEND=demo ./scripts/dev.sh start   # no sudo, simulated network/firewall
```

`ANTH_BACKEND=demo` uses a separate run directory `/tmp/anthracite-panel/demo`
so it cannot mix state with a real development run. It starts the daemon as the
calling user, without `sudo`, and sets `ANTH_BACKEND=demo` for the process.

The dev helper prints a warning: network, firewall, files and processes are
simulated in this slice. Do not use it as a safety boundary yet; account,
storage and terminal operations still target the host.

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
- Snapshots use the same wire types, so the existing UI and CLI work unchanged.

The state is shared by all clients and lost on restart. Per-session state,
feature toggles, the demo banner, reset endpoint, virtual files/terminal and the
separate hosted image are the next phases in
`/root/.opencode/plan/anthracite-demo.md`.

## Host boundary

`network` and `firewall` now live in `anth-host`:

```text
anth-host/src/network
anth-host/src/firewall
anth-host/src/backend.rs   traits used by anth-panel
anth-host/src/demo         virtual implementations
```

`anth-panel` keeps policy, guards, dispatch, HTTP/CLI rendering and UI. It talks
to `NetworkBackend` and `FirewallBackend`, so the same request path serves the
real and demo implementations.
