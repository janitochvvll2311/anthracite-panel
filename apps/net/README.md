# Network

Network is a native host network service plus the sandboxed panel view. The demo
instance uses the shared scripted fixture from `demo/emulator`; this directory
builds the real app.

- `backend/` reads `/sys/class/net`, `getifaddrs(3)`, `/proc/net/route` and
  `/proc/net/ipv6_route`. It reports interfaces (up flag, MAC, MTU, addresses,
  counters) and the IPv4/IPv6 routing tables.
- `set-up`, `add-route` and `remove-route` validate the interface, family, CIDR
  and gateway, then run `ip`. They need `CAP_NET_ADMIN`; without it the call
  returns a typed `denied` error.
- `frontend/` is the network view: interface cards, the route table and the
  up/down and route controls.

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

## Testing the apply flow

A normal process cannot change interfaces or routes, so the privileged apply
test is deferred in unprivileged environments. `demo/emulator` implements the
same read/apply surface over its fixture: the demo Network view exercises
up/down, add-route and remove-route, and the validation rules match this
service.
