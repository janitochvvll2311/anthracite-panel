# Network interfaces

The daemon can inspect every network interface on the host it runs on and,
when explicitly enabled, bring one up or down. Monitoring is read-only and works
without any configuration.

The panel has no authentication yet, so keep the HTTP port on a trusted network
or behind a proxy that authenticates users. Interface names, MAC addresses and
IP addresses are visible to anyone who can reach the API.

## Configuration

Network control is **off by default**:

```toml
[network]
control = false
```

Set it to `true` to allow up/down requests:

```toml
[network]
control = true
```

Changing the file requires a daemon restart.

Debug builds (`cargo run`) bootstrap a missing config with `control = true` so
the feature can be tested locally. Release builds generate `false`, and an
existing file is never rewritten.

Network control is limited to administrative state:

- no MTU, MAC or IP-address changes;
- no routing or firewall changes;
- loopback interfaces cannot be brought down.

Changing an interface can disconnect the host. The CLI asks for confirmation
unless `--yes` is given, and refuses to run non-interactively without it.

## CLI

```bash
anth-panel network list
anth-panel network ls                  # alias
anth-panel network list --watch 2s
anth-panel network up eth0
anth-panel network down eth0
anth-panel network down eth0 --yes
```

`list` prints IFACE, STATE, ADDRESS, SPEED, RX and TX. Traffic columns are
cumulative counters; `--watch` redraws them but does not compute rates.

`up` and `down` return exit code `1` when the daemon refuses the change or the
operation fails. The daemon must be running as a user with `CAP_NET_ADMIN`
(usually root) for a state change to succeed.

## Data sources

- `/sys/class/net/<name>` for the interface name, index, state, flags, MAC, MTU,
  speed and duplex.
- `/proc/net/dev` for the RX/TX byte, packet, error and drop counters.
- `getifaddrs(3)` for IPv4 and IPv6 addresses and prefix lengths.

No external command such as `ip` is required. The parser takes a filesystem root
in tests, so fixtures can exercise the sysfs and `/proc` paths without touching
the host.

## Data model

Each snapshot contains `interfaces`, `timestamp_ms`, `control_enabled`, `total`,
`truncated` and an optional `warning`.

| Field | Meaning |
| --- | --- |
| `name` | Interface name |
| `index` | Kernel interface index |
| `state` | Operational state from sysfs (`up`, `down`, `unknown`, ...) |
| `up` | Administrative `IFF_UP` flag |
| `running` | Carrier/running `IFF_RUNNING` flag |
| `loopback` | Loopback `IFF_LOOPBACK` flag |
| `mac` | Hardware address |
| `mtu` | Maximum transmission unit |
| `speed_mbps` | Negotiated link speed, or `null` when the driver has none |
| `duplex` | `full`, `half`, or `null` |
| `addresses` | IPv4/IPv6 addresses with a prefix length |
| `rx_bytes`, `tx_bytes` | Cumulative byte counters |
| `rx_packets`, `tx_packets` | Cumulative packet counters |
| `rx_errors`, `tx_errors` | Cumulative error counters |
| `rx_dropped`, `tx_dropped` | Cumulative drop counters |

Rates are **computed by the client** from two snapshots:

```text
rx/s = (rx_bytes_now - rx_bytes_before) / seconds_between_snapshots
tx/s = (tx_bytes_now - tx_bytes_before) / seconds_between_snapshots
```

A counter that goes backwards, a missing previous sample or a non-positive time
delta produces no rate rather than a bogus value.

## Truncation

One protocol line is capped at 1 MiB. A snapshot returns at most 2000
interfaces, sorted by name. When the host has more, `truncated` is `true`,
`total` reports the number found, and `warning` contains a human-readable
notice. The CLI prints the warning below the table and the web component shows
it above the table.

## HTTP and browser

The endpoints are documented in [api-endpoints.md](api-endpoints.md):

- `GET /api/network`
- `GET /api/network/watch?interval=2s`
- `POST /api/network/{name}/state` with `{"up":true}`

The browser component lives in `frontend/src/components/anth-network-monitor.ts`.
It follows the SSE stream through an injected `NetworkClient`, computes rates,
sorts and filters rows, and shows per-interface details. When the snapshot says
`control_enabled`, each row offers an up/down action; the component confirms
before sending it.

`frontend/src/components/anth-network-monitor.test.html` is the dev-only
harness. The same behaviour is covered by the Vitest tests under
`frontend/src/components/` and `frontend/src/lib/network/`.
