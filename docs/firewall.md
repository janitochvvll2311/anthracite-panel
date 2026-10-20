# Firewall

The daemon can inspect and manage a dedicated nftables table. Monitoring is
read-only; adding, deleting and flushing rules requires explicit control.

The panel has no authentication yet, so keep the HTTP port on a trusted network
or behind a proxy that authenticates users. Anyone who can reach the API can
change firewall rules when control is enabled.

## Requirements

- Linux with nftables.
- The `nft` binary on `PATH`.
- `CAP_NET_ADMIN` for the daemon user. Running the daemon as root also works.

For a host development install, grant the capability to the nft binary once as
root:

```bash
setcap cap_net_admin+ep "$(command -v nft)"
```

The Docker image applies the same `setcap` during the build.

Anthracite does **not** read or modify other firewall tables. It owns:

```text
inet anthracite
└── input (type filter hook input priority filter; policy accept)
```

Deny rules created by the panel are tagged with an `anth:` comment, which lets
the daemon recover their action, protocol and port from nftables.

The managed chain is a **blocklist** with an accept policy. `deny` adds a `drop`
rule; `allow` removes a matching managed `drop` rule. This keeps the managed
chain a pure blocklist: an `accept` rule would only end this chain and cannot
override another firewall table, which still applies.

## Configuration

Firewall control is **off by default**:

```toml
sudo = false

[firewall]
control = false
```

Set `control` to `true` to allow changes. The top-level `sudo = true` covers
both firewall and network control commands; it runs every `nft` invocation as
`sudo -n nft` and is meant for development when the daemon runs as an
unprivileged user with a `NOPASSWD` sudoers rule for `nft`:

```toml
sudo = true

[firewall]
control = true
```

Passwords are never stored. If sudo is missing, requires a password or is not
allowed, the daemon retries `nft` directly so a file capability or a root daemon
still works. Otherwise grant the nft binary `CAP_NET_ADMIN`, run the daemon as
root, or use the container capability described below.

The legacy `firewall.sudo` and `firewall.sudo_password` keys are still accepted
so older configs keep loading, but they are ignored at runtime and never
written back.

Changing the file requires a daemon restart.

Debug builds (`cargo run`) bootstrap a missing config with global `sudo = true`
and `control = true` so the feature can be tested locally. Release builds
generate `false` for both, and an existing file is never rewritten.

## CLI

```bash
anth-panel firewall status
anth-panel firewall list              # alias

anth-panel firewall allow 22/tcp
anth-panel firewall allow 53/udp
anth-panel firewall allow 8080        # tcp by default
anth-panel firewall deny 23/tcp

anth-panel firewall delete 17
anth-panel firewall flush
```

- `status` prints the managed rules as a table and works with control disabled.
- `allow` removes a matching managed deny rule; `deny` adds a drop rule.
- `allow`, `deny`, `delete` and `flush` ask for confirmation on a TTY.
- `--yes` skips the confirmation and is required when stdin is not a terminal.
- `flush` removes only rules from `inet anthracite input`.

Ports must be between `1` and `65535`. The optional protocol suffix is `tcp` or
`udp`. Comments are limited to 32 characters from `A-Z`, `a-z`, `0-9`, `.`, `_`
and `-`.

If `allow` finds no matching managed deny, it reports that traffic is already
allowed by the chain policy.

A daemon that cannot run `nft`, lacks permission or has control disabled exits
`1` with the daemon's error.

## Data model

Each snapshot contains:

| Field | Meaning |
| --- | --- |
| `initialized` | Whether the `inet anthracite` table exists |
| `control_enabled` | Whether `[firewall] control` allows changes |
| `rules` | Rules found in the managed table |
| `ruleset` | Textual `nft list table inet anthracite` output |

Each rule has:

| Field | Meaning |
| --- | --- |
| `handle` | nftables rule handle, used by `delete` |
| `chain` | Chain containing the rule |
| `action` | `allow`, `deny`, or `custom` for a rule not created by the panel |
| `protocol` | `tcp`, `udp`, or `null` |
| `port` | Port number, or `null` |
| `comment` | Panel comment or the raw comment for custom rules |

## HTTP and browser

The endpoints are documented in [api-endpoints.md](api-endpoints.md):

- `GET /api/firewall`
- `POST /api/firewall/rules`
- `POST /api/firewall/rules/{handle}/delete`
- `POST /api/firewall/flush`

Example:

```json
{"action":"allow","port":8080,"protocol":"tcp","comment":"web"}
```

The browser component lives in `frontend/src/components/anth-firewall.ts`. It
shows the managed rules, adds allow/deny rules and confirms deletes and flushes.
It only depends on the injected `FirewallClient`; the HTTP implementation is in
`frontend/src/lib/firewall/firewall-client.ts`.

`frontend/src/components/anth-firewall.test.html` is the dev-only harness.
`frontend/src/components/component-index.test.html` links every component
harness.

## Container

The runtime image includes `nftables` and grants `CAP_NET_ADMIN` to `/usr/sbin/nft`
with a file capability. The container still needs the capability in its bounding
set.

The repository's `compose.yaml` already adds it:

```bash
docker compose up --build
```

The equivalent direct invocation is:

```bash
docker run --cap-add NET_ADMIN -p 8080:8080 anth-panel
```

With Podman:

```bash
podman run --cap-add NET_ADMIN -p 8080:8080 anth-panel:local
```

Without `CAP_NET_ADMIN`, status and mutations fail with a permission error. If
the file capability is unavailable in the runtime, run the container as root
with the same capability:

```bash
docker run --user 0 --cap-add NET_ADMIN -p 8080:8080 anth-panel
```
