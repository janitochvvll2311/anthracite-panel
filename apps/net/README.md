# Network

Network is a native read-only host network service plus the sandboxed panel
view. The demo instance uses the shared scripted fixture from `demo/emulator`;
this directory builds the real app.

- `backend/` reads `/sys/class/net`, `getifaddrs(3)`, `/proc/net/route` and
  `/proc/net/ipv6_route`. It reports interfaces (up flag, MAC, MTU, addresses,
  counters) and the IPv4/IPv6 routing tables.
- `frontend/` is the network view: interface cards and the route table.
- `set-up`, `add-route` and `remove-route` are refused with a typed `denied`
  error. Applying configuration is the separate approved flow (decision P4); the
  read-only slice never mutates the host.

## Build

```bash
apps/net/build.sh
# writes apps/net/dist/net.anth
```

The script builds and type-checks the frontend, builds `net-service`, hashes the
binary and module, and packages `manifest.toml`, `wit/net.wit`, `web/net.js` and
`bin/net-service`.

## Install and approve

The real service declares `class = "host"`, so approval needs the full risk
ladder:

```bash
ANTH_SOCKET=/path/to/engine.sock
./target/debug/anth --socket "$ANTH_SOCKET" app upgrade org.anthracite.net \
    apps/net/dist/net.anth
./target/debug/anth --socket "$ANTH_SOCKET" app approve org.anthracite.net \
    --accept-write --accept-admin --accept-host
```

It replaces the emulator bundle under the same app id. Restart the panel after
the upgrade so the new service starts.
