# Network

The daemon reads the host's network inventory from the kernel and reports
interfaces, addresses, link counters and default routes. This slice is
read-only; bringing an interface up or down, creating, editing, renaming and
deleting interfaces, and the firewall arrive with the next cut.

Reading works with the normal panel session. The inventory describes the
host's links and addresses, so keep the HTTP port on a trusted network or
behind an authenticating proxy.

## Configuration

```toml
[network]
control = false
manage = false
```

- `control` will gate bringing an interface up or down. It is reported in every
  snapshot but has no effect on the inventory.
- `manage` will gate creating, editing, renaming and deleting interfaces. It is
  reported in every snapshot for the same reason.
- Both default to off. A change requires a daemon restart; the running daemon
  keeps the policy it read at startup.
- Debug builds bootstrap a missing config with both switches on. Release builds
  generate the defaults above and never rewrite an existing file.

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

`kind` is one of `loopback`, `ether`, `wifi`, `bridge`, `tun`, `veth` or
`other`. It is derived in that order: the loopback flag, the wireless
attributes, the bridge directory, a `veth` name, a `tun`/`tap` name, the ARPHRD
type (`1` is Ethernet) and finally `other`.

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
      "loopback": false,
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

## Socket

```json
{ "type": "network" }
```

The answer is:

```json
{ "type": "network", "data": { "interfaces": [], "routes": [], "timestamp_ms": 0, "control_enabled": false, "manage_enabled": false } }
```

The socket request goes through the same `Daemon::execute` path as HTTP, so
neither transport can answer differently.

## CLI

```bash
anth-panel network
anth-panel network --watch
anth-panel network --watch 2s
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

`--watch` repeats the table until Ctrl-C. A bare `--watch` uses `2s`. The CLI
does not compute rates: it prints the cumulative counters exactly as the daemon
reported them.

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

There are no mutation controls yet: the daemon reports `control_enabled` and
`manage_enabled`, and the next slice uses them.
