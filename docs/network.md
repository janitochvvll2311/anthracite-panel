# Network interfaces

The daemon can inspect every network interface on the host it runs on, and when
explicitly enabled bring one up or down or create, edit, rename and delete
interfaces. Monitoring is read-only and works without any configuration.

The panel has no authentication yet, so keep the HTTP port on a trusted network
or behind a proxy that authenticates users. Interface names, MAC addresses and
IP addresses are visible to anyone who can reach the API.

## Configuration

Network control is **off by default**:

```toml
sudo = false

[network]
control = false
manage = false
```

Set `control` to `true` to allow up/down requests, and `manage` to `true` to
allow create, edit, rename and delete. The two switches are separate on purpose:
an operator who enabled up/down has not asked for an interface that can be
deleted. The top-level `sudo = true`
covers both network and firewall control commands; it tries
`sudo -n ip link set` first, which is meant for development when the daemon runs
as an unprivileged user with a `NOPASSWD` sudoers rule for `ip`:

```toml
sudo = true

[network]
control = true
manage = true
```

If sudo is missing, requires a password or is not allowed, the daemon retries
the in-process ioctl so `CAP_NET_ADMIN` or a root daemon still works. Passwords
are never stored. The loopback-down guard runs before the sudo command.

The legacy `network.sudo` key is still accepted so older configs keep loading,
but it is ignored at runtime and never written back.

Changing the file requires a daemon restart.

Debug builds (`cargo run`) bootstrap a missing config with global `sudo = true`,
`control = true` and `manage = true` so the feature can be tested locally.
Release builds generate `false` for all three, and an existing file is never
rewritten.

Network control is limited to administrative state:

- no MAC or IP-address changes;
- no routing or firewall changes;
- loopback interfaces cannot be brought down or deleted.

## Management guards

Create, edit, rename and delete all validate against the host before they run
`ip`. The guards exist because a panel that can remove the link it is served
over locks its own operator out.

**Create** only accepts virtual kinds: `veth`, `bridge`, `bond`, `vlan`, `dummy`
and `tun/tap`. A physical device cannot be created. A `veth` needs a peer name
and a `vlan` needs an existing parent plus an identifier between 1 and 4094.
Names are limited to 15 bytes with no whitespace, slash or colon; MTUs must fall
between 68 and 65535.

**Edit** changes the MTU and/or the administrative state. A field that the
request omits is left alone, so sending only `mtu` never takes a link down. An
edit with neither field is refused as "nothing to change".

**Rename** is refused while the link is up, because the kernel refuses it too
and forcing it would stop traffic for a reason the operator did not ask for.

**Delete** is allowed for physical devices. The panel warns that the host loses
that link rather than hiding the action, because removing a card on purpose is a
legitimate operation. Three guards still refuse the delete:

- a loopback interface;
- the interface the HTTP request arrived through, resolved from the connection's
  peer address against the interface prefixes (longest match wins);
- the last interface carrying a default route.

The peer guard is skipped when the address says nothing about the path: a
loopback peer means the client is on this host, and an address matching no
interface is a proxy or a NAT hop. Skipping it is deliberate; guessing would
either block a legitimate delete or claim a protection it does not provide. The
routes are read with `ip route show default` for IPv4 and IPv6 on every mutation
rather than cached, so a route that appeared since the last snapshot is visible
to the guard.

`interface.kind` is derived from sysfs: a `device` link means real hardware, and
`bridge/`, `bonding/`, `vlan_id`, `tun_flags`, or an `iflink` that differs from
`ifindex` identify the virtual kinds. `physical` is `true` only for
`kind: physical`.

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
anth-panel network create br0 --kind bridge --mtu 1500 --up
anth-panel network create veth0 --kind veth --peer veth1
anth-panel network create eth0.10 --kind vlan --link eth0 --vlan-id 10
anth-panel network edit eth0 --mtu 9000
anth-panel network edit eth0 --down
anth-panel network rename d0 d1 --yes
anth-panel network delete d0 --yes
```

`list` prints IFACE, STATE, ADDRESS, SPEED, RX and TX. Traffic columns are
cumulative counters; `--watch` redraws them but does not compute rates.

Create, edit, rename and delete ask for confirmation unless `--yes` is given.
They return exit code `1` when the daemon refuses the change or the operation
fails; `up` and `down` behave the same way. The daemon needs `CAP_NET_ADMIN`, root, or global `sudo = true`
with a passwordless sudoers rule for `ip`.

## Data sources

- `/sys/class/net/<name>` for the interface name, index, state, flags, MAC, MTU,
  speed and duplex.
- `/proc/net/dev` for the RX/TX byte, packet, error and drop counters.
- `getifaddrs(3)` for IPv4 and IPv6 addresses and prefix lengths.

The default state-change path uses the `SIOCSIFFLAGS` ioctl and needs no
external command. With global `sudo = true`, the daemon first tries
`sudo -n ip link set`; a missing or unusable sudo/ip falls back to the ioctl.
The parser takes a filesystem root in tests, so fixtures can exercise the sysfs
and `/proc` paths without touching the host.

## Data model

Each snapshot contains `interfaces`, `timestamp_ms`, `control_enabled`,
`manage_enabled`, `total`, `truncated` and an optional `warning`.

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
| `kind` | `physical`, `loopback`, `bridge`, `bond`, `vlan`, `veth`, `dummy`, `tun_tap`, `wireless` or `other` |
| `physical` | The interface has a `device` link in sysfs |

Rates are **computed by the client** from two snapshots:

```text
rx/s = (rx_bytes_now - rx_bytes_before) / seconds_between_snapshots
tx/s = (tx_bytes_now - tx_bytes_before) / seconds_between_snapshots
```

A counter that goes backwards, a missing previous sample or a non-positive time
delta produces no rate rather than a bogus value.

## Truncation

One protocol line is capped at 1 MiB. A snapshot returns at most 2000
interfaces, sorted by name, and is shrunk further when address-heavy rows still
exceed the line cap. When any rows are dropped, `truncated` is `true`, `total`
reports the number found, and `warning` contains a human-readable notice. The
CLI prints the warning below the table and the web component shows it above the
table.

## HTTP and browser

The endpoints are documented in [api-endpoints.md](api-endpoints.md):

- `GET /api/network`
- `GET /api/network/watch?interval=2s`
- `POST /api/network/{name}/state` with `{"up":true}`
- `POST /api/network` with the create body
- `PATCH /api/network/{name}` with `{"mtu":9000}` or `{"up":false}`
- `POST /api/network/{name}/rename` with `{"new_name":"d1"}`
- `POST /api/network/{name}/delete`

The browser component lives in `frontend/src/components/anth-network-manager.ts`.
It follows the SSE stream through an injected `NetworkClient`, computes rates,
sorts and filters rows, and shows per-interface details. When the snapshot says
`control_enabled`, each row offers an up/down action; the component confirms
before sending it.

`frontend/src/components/anth-network-monitor.test.html` is the dev-only
harness. The same behaviour is covered by the Vitest tests under
`frontend/src/components/` and `frontend/src/lib/network/`.
