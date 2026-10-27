# Network

The daemon reads the host's network inventory from the kernel and reports
interfaces, addresses, link counters and default routes. With the policy
switches on it also brings an interface up or down, creates a virtual
interface, changes its MTU or state, renames it and deletes it.

The daemon runs `ip` directly with its own privileges, as an argument vector,
never through a shell. It does not use `sudo`: run the daemon as the account
that should hold network privileges, and leave the switches off otherwise.

Reading works with the normal panel session. The inventory describes the
host's links and addresses, so keep the HTTP port on a trusted network or
behind an authenticating proxy.

## Configuration

```toml
[network]
control = false
manage = false
```

- `control` gates `network up` / `network down` and the two state buttons in the
  UI. It is independent from `manage`.
- `manage` gates create, edit, rename and delete, including an edit that changes
  the administrative state.
- Both default to off. A change requires a daemon restart; the running daemon
  keeps the policy it read at startup.
- Debug builds bootstrap a missing config with both switches on. Release builds
  generate the defaults above and never rewrite an existing file.
- The inventory is always readable, whichever switch is on. The flags are
  reported in every snapshot so a client can hide controls it cannot use.

### Runtime requirements

- Mutations need `ip` from **iproute2**, not the BusyBox applet. BusyBox
  implements `ip link set`/`show`, `addr` and `route`, but not the `ip link
  add`, `ip link delete` or `ip tuntap` forms the panel uses. The daemon reports
  `501 ip_unsupported` for a command the local `ip` does not understand.
- The daemon's process needs `CAP_NET_ADMIN`. Being uid 0 is not enough inside a
  container whose bounding set excludes it. In Docker, start the container with
  `--cap-add=NET_ADMIN` (or run the daemon on the host with the right account).
- `tun` creation additionally needs `/dev/net/tun`. In Docker, pass
  `--device /dev/net/tun`; a missing device is `501 tun_unavailable`.
- The firewall cut will add `nftables` and the same capability. It is not needed
  for the inventory or interface mutations.
- A missing `ip` is `500 ip_unavailable`; a refusal from `ip` is `403
  ip_permission` or another kind from its stderr.

`ip -V` prints `ip utility, iproute2-...` when the right binary is in `PATH`.
`grep CapEff /proc/self/status` should contain bit 12 (`0x1000`) for the daemon
process.

## Sources

The scan is one pass over the kernel's own views:

| Source | Used for |
| --- | --- |
| `/sys/class/net/<name>/ifindex` | Interface index |
| `/sys/class/net/<name>/flags` | `IFF_UP`, `IFF_LOOPBACK` |
| `/sys/class/net/<name>/operstate` | Operational state |
| `/sys/class/net/<name>/mtu`, `address`, `speed`, `type` | MTU, MAC, link speed and ARPHRD type |
| `/sys/class/net/<name>/wireless`, `phy80211`, `bridge` | Kind classification |
| `getifaddrs` | IPv4 and IPv6 addresses with their prefix lengths |
| `/proc/net/dev` | Byte, packet, error and drop counters |
| `/proc/net/route` | Lowest-metric IPv4 default route |
| `/proc/net/ipv6_route` | Lowest-metric IPv6 default route |

The scan runs on the blocking pool. A file that disappears between `read_dir`
and the read is skipped; a counter that cannot be parsed is `0`. An interface
without a MAC (loopback) reports an empty string.

### Kinds

`kind` is one of `loopback`, `ether`, `wifi`, `bridge`, `bond`, `vlan`, `tun`,
`veth`, `dummy` or `other`. It is derived in that order from the loopback flag,
the wireless attributes, the bridge and bonding directories, the `DEVTYPE=vlan`
uevent, a `veth`/`tun`/`tap`/`dummy` name, the ARPHRD type (`1` is Ethernet) and
finally `other`.

`physical` is true when `/sys/class/net/<name>/device` exists, so the UI can
warn that a delete removes a real card. `running` is `IFF_RUNNING`, independent
of `up`.

### Addresses

Each address carries:

| Field | Meaning |
| --- | --- |
| `family` | `inet` or `inet6` |
| `address` | Canonical address text, without a zone suffix |
| `prefix` | Prefix length, not a dotted netmask |
| `scope` | `host`, `link` or `global` |

Loopback addresses are `host`; `169.254.0.0/16` and `fe80::/10` are `link`;
everything else is `global`. Addresses are sorted by family and text and
deduplicated.

## Rates

The counter fields (`rx_bytes`, `tx_bytes`, packets, errors, drops) are
cumulative. The daemon never guesses a rate: the web client computes it from two
consecutive snapshots and their `timestamp_ms`:

```text
rate = (now_bytes - previous_bytes) / ((now_ms - previous_ms) / 1000)
```

A missing previous sample, a non-positive interval or a counter that went
backwards (the interface was recreated) shows `—` instead of a negative rate.
Pausing or reopening the stream drops the baseline.

## Data model

```json
{
  "interfaces": [
    {
      "name": "eth0",
      "index": 2,
      "kind": "ether",
      "mac": "00:11:22:33:44:55",
      "mtu": 1500,
      "state": "up",
      "up": true,
      "running": true,
      "loopback": false,
      "physical": true,
      "addresses": [
        { "family": "inet", "address": "192.168.1.10", "prefix": 24, "scope": "global" },
        { "family": "inet6", "address": "fe80::1", "prefix": 64, "scope": "link" }
      ],
      "rx_bytes": 123456,
      "tx_bytes": 654321,
      "rx_packets": 1000,
      "tx_packets": 900,
      "rx_errors": 0,
      "tx_errors": 0,
      "rx_dropped": 0,
      "tx_dropped": 0,
      "speed_mbps": 1000
    }
  ],
  "routes": [
    { "family": "inet", "gateway": "192.168.1.1", "interface": "eth0" }
  ],
  "timestamp_ms": 1790000000000,
  "control_enabled": false,
  "manage_enabled": false
}
```

`speed_mbps` is omitted when the driver reports no positive speed. The daemon
sorts interfaces by index and then name.

## Mutations

Every mutation re-reads the live inventory before it checks a guard, so a change
cannot be applied to an interface that disappeared or came up since the operator
looked. `ip` runs directly with an argv vector: no shell, no `eval`, no
interpolation and no `sudo`. The daemon maps a refusal to `forbidden`
(`ip_permission`), `not_found`, `conflict` (`interface_exists`) or `invalid`
(`ip_refused`), and keeps `ip`'s stderr in the message.

### Up and down

```json
{ "name": "eth0", "up": false }
```

Requires `[network] control`. A loopback cannot be brought down. Nothing else
is guarded here: taking down a link can disconnect the operator, so the action
is explicit in both transports and the CLI is expected to be used deliberately.

### Create

```json
{
  "name": "br0",
  "kind": "bridge",
  "mtu": 1500,
  "up": false
}
```

Requires `[network] manage`. The accepted kinds are `bridge`, `bond` (mode
`active-backup`), `dummy`, `veth` (needs `peer`), `vlan` (needs `link` and
`vlan_id` 1-4094) and `tun` (a persistent `tap` device). Physical, wireless and
loopback interfaces cannot be created.

The name is 1-15 bytes with no whitespace, `/`, `:` or NUL. The MTU is 68-65535
when present. The name and a veth peer must not already exist; a vlan parent
must. The daemon runs the create, then the optional MTU and up commands. If a
follow-up fails, the error says the interface may already have been created
instead of pretending the whole operation rolled back.

### Edit

```json
{ "name": "eth0", "mtu": 1400, "up": false }
```

Requires `[network] manage`; changing `up` also requires `[network] control`.
At least one of `mtu` and `up` is required. An absent field is left alone, so
changing the MTU never changes the state by omission. Bringing a loopback down
is refused.

### Rename

```json
{ "name": "eth0", "new_name": "lan0" }
```

Requires `[network] manage` and an interface that is already down; an up
interface is `conflict` / `interface_up`. The new name follows the same rules
and must not exist.

### Delete

```json
{ "name": "eth0" }
```

The HTTP route supplies `peer` from the connection; the wire request also
carries it when the transport knows it. Requires `[network] manage`. The daemon
refuses:

- a loopback interface;
- the only interface carrying a default route;
- the interface the HTTP request's peer address is routed through, so the panel
  cannot delete the link it is served over. A loopback peer, a proxy address
  that matches no interface, and a socket request without a peer skip this
  guard, because the daemon cannot know the path.

A physical interface is allowed after the warning: removing a card may be
exactly what the operator wants. The guard list is computed from a fresh
snapshot on every request.

## HTTP

All routes require a live panel session unless auth is disabled.

### `GET /api/network`

Returns one snapshot in the shape above. It never reads or mutates a password,
so it is not rate-limited.

### `GET /api/network/watch?interval=2s`

Server-Sent Events, the same shape as `host-info/watch`:

- `event: network`
- `id`: `timestamp_ms`
- `data`: one snapshot
- `retry: 3000`
- Keep-alive comments every 15 seconds.

A bad interval is a `400` before the stream opens. `interval` accepts `ms`,
`s`, `m` and `h` suffixes and is at least `10ms`.

### Mutations

All five routes are `POST`, require a live session, and answer
`200 {"message":"..."}`. Their bodies are the shapes in **Mutations** above:

| Route | Policy |
| --- | --- |
| `/api/network/state` | `control` |
| `/api/network/create` | `manage` |
| `/api/network/edit` | `manage` |
| `/api/network/rename` | `manage` |
| `/api/network/delete` | `manage`; the server adds the peer address |

The delete route is the only one that reads `ConnectInfo`; without it (a test
router) the peer is unknown and that one guard is skipped. It never trusts a
peer value from the JSON body.

## Socket

```json
{ "type": "network" }
```

The answer is:

```json
{ "type": "network", "data": { "interfaces": [], "routes": [], "timestamp_ms": 0, "control_enabled": false, "manage_enabled": false } }
```

The mutation requests use the same dispatch as HTTP:

```json
{ "type": "network_set_state", "data": { "name": "eth0", "up": false } }
{ "type": "network_create", "data": { "name": "br0", "kind": "bridge" } }
{ "type": "network_edit", "data": { "name": "eth0", "mtu": 1400 } }
{ "type": "network_rename", "data": { "name": "eth0", "new_name": "lan0" } }
{ "type": "network_delete", "data": { "name": "eth0" } }
```

Each answers `{"type":"network_changed","data":{"message":"..."}}`. The socket
has no peer address, so the delete guard for the request path is skipped; the
loopback and last-default-route guards still apply. The socket request goes
through the same `Daemon::execute` path as HTTP, so neither transport can answer
differently.

## CLI

```bash
anth-panel network list
anth-panel network ls --watch
anth-panel network up eth0
anth-panel network down eth0
anth-panel network create br0 --kind bridge --mtu 1500 --up --yes
anth-panel network edit eth0 --mtu 1400 --yes
anth-panel network edit eth0 --down --yes
anth-panel network rename eth0 lan0 --yes
anth-panel network delete eth0 --yes
```

The table prints one row per address, with the counters on the first row:

```text
INTERFACE       STATE    KIND       MTU  ADDRESS                                         RX         TX
lo              unknown  loopback 65536  127.0.0.1/8                              729.7 MiB  729.7 MiB
                                         ::1/128
eth0            up       ether     1500  172.21.0.2/16                              1.4 GiB    7.0 GiB

DEFAULT ROUTES
  inet via 172.21.0.1 dev eth0
```

`list` (alias `ls`) prints the table; `--watch` repeats it until Ctrl-C and a
bare `--watch` uses `2s`. The CLI does not compute rates: it prints the
cumulative counters exactly as the daemon reported them.

`up` and `down` need `[network] control`. `create`, `edit`, `rename` and
`delete` need `[network] manage`; `edit --up/--down` also needs `control`.
They ask for confirmation on a TTY and refuse without `--yes` when stdin is not
a terminal. `create` accepts `--kind` (`veth`, `bridge`, `bond`, `vlan`,
`dummy` or `tun-tap`), `--peer`, `--link`, `--vlan-id`, `--mtu` and `--up`.
`edit` takes `--mtu`, `--up` or `--down`.
Mutations print the daemon's one-line message.

## Web UI

**Network** is one section in the sidebar with an **Interfaces** tab. The tab
owns its SSE feed and closes it when the section is left:

- Toolbar: feed state, interface/up count, refresh interval and a
  pause/resume/retry button.
- Default routes: one chip per family, or `none`.
- Filter: matches name, kind, state, MAC and address text.
- One card per interface: name, kind and state, link speed, MTU and MAC,
  address chips, RX/TX rates and totals. The first frame shows `—` for rates.
- A down interface keeps the red/neutral accent; an interface with errors or
  drops shows their counts.
- On narrow screens the card becomes one column and the counters move under
  the identity.

### Mutation controls

- With `control`, each card gets an **Up** or **Down** button. A loopback's
  **Down** is disabled with the reason; the same is true for a loopback delete.
- With `manage`, the toolbar gets **Create** and each card gets **Edit MTU**,
  **Rename** and **Delete**. A rename is disabled while the interface is up and
  a delete is disabled on the last default-route carrier; both carry their
  reason in `title`.
- **Create** opens one form with name, kind, kind-specific fields, MTU and
  **Bring up**. The fields switch between veth (peer), vlan (parent and VLAN
  id) and the rest. The name is not trusted: the daemon validates it.
- **Edit MTU** and **Rename** open one inline form inside the card. **Delete**
  opens an inline confirmation; a physical interface adds the warning that the
  host will lose the link.
- One mutation runs at a time. A success refreshes the snapshot and shows
  `Change applied.`; a refusal keeps the card and shows the daemon message
  above the list. The live stream is not restarted.
- Controls are absent, not disabled, when the corresponding policy is off.
