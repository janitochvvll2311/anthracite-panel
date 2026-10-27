# Firewall

The daemon manages exactly one nftables table and one chain:

```text
table inet anthracite
└── chain input (type filter hook input priority filter; policy accept)
```

Everything the panel reads or changes is scoped to that chain. `nft` is invoked
directly with an argument vector, never through a shell, and the daemon never
touches another table.

The chain is a **blocklist with an accept policy**. Denying a port adds a
`drop` rule; allowing it removes a matching managed `drop` rule instead of
adding an `accept` rule. An accept verdict in this table would only end this
chain and cannot override a drop in another table, so it would be misleading.

## Configuration

```toml
[firewall]
control = false
```

`control` gates every mutation. Status is readable with it off, so an operator
can inspect the rules without being able to change them. The switch is reported
in every snapshot. A change requires a daemon restart; the running daemon keeps
the policy it read at startup.

Debug builds bootstrap a missing config with it on. Release builds generate the
default above.

## Requirements

- `nft` from nftables on `PATH`.
- `CAP_NET_ADMIN` in the daemon's process. Being uid 0 inside a container is
  not enough when the bounding set excludes it. In Docker, use
  `--cap-add=NET_ADMIN`.
- A missing binary is `501 nft_unavailable`; a permission refusal is `403
  nft_permission`.

These are the same runtime requirements as network mutations; see
[network.md](network.md#runtime-requirements).

## Data model

One rule:

| Field | Meaning |
| --- | --- |
| `handle` | nftables rule handle, used by delete |
| `chain` | Always `input` |
| `action` | `allow`, `deny`, or `custom` for a rule the panel did not create |
| `protocol` | `tcp` or `udp`, absent for a custom rule |
| `port` | 1–65535, absent for a custom rule |
| `comment` | User comment for managed rules, or the raw nft comment for custom rules |

A snapshot also carries `initialized` (the table and chain exist),
`control_enabled`, the textual `ruleset` for diagnostics, and `timestamp_ms`.
A missing table is a normal empty state, not an error: `initialized: false`,
no rules and an empty ruleset.

## Rule comments

Every managed rule carries:

```text
anth:<action>:<protocol>:<port>[|<comment>]
```

For example, `anth:deny:tcp:80|http`. The prefix is how the daemon recovers the
intent and the operator comment from nft itself; there is no side database.

A comment the daemon cannot parse becomes `custom` and is shown with its raw
text. Custom rules in the `input` chain are still listed and can be deleted,
because the chain belongs to the panel. Rules in another chain are not managed
and are ignored.

A user comment is optional, at most 32 characters, and limited to ASCII
letters, digits, `.`, `_` and `-`. This keeps the nft argument free of quoting
tricks; the daemon still passes it as one argv element.

## Mutations

All mutations require `[firewall] control` and answer with a one-line message.

### Deny

```json
{ "action": "deny", "protocol": "tcp", "port": 80, "comment": "http" }
```

Validates the port and comment, creates the table and chain if they are missing,
then runs:

```text
nft add rule inet anthracite input tcp dport 80 drop comment "anth:deny:tcp:80|http"
```

### Allow

The same body with `"action": "allow"`. It removes every managed `deny` rule
matching that protocol and port. If the table is missing or no managed deny
matches, nothing changes and the answer says traffic is already allowed.

### Delete

```json
{ "handle": 4 }
```

Deletes one rule from `inet anthracite input` by handle. The handle is scoped to
the input chain, so a handle from another chain cannot be reached. A zero
handle is `400 firewall_handle`.

### Flush

Removes every rule from the `input` chain, leaving the table and chain in place.
An uninitialized table is a no-op with a message. The response counts the rules
that were present.

## HTTP

All routes require a live panel session unless auth is disabled.

### `GET /api/firewall`

Returns the snapshot. A readable table with no rules has `initialized: true`
and `rules: []`.

```json
{
  "initialized": true,
  "control_enabled": true,
  "rules": [
    {
      "handle": 3,
      "chain": "input",
      "action": "deny",
      "protocol": "tcp",
      "port": 80,
      "comment": "http"
    }
  ],
  "ruleset": "table inet anthracite { ... }",
  "timestamp_ms": 1790000000000
}
```

### `POST /api/firewall/rule`

```json
{ "action": "deny", "protocol": "tcp", "port": 80, "comment": "http" }
```

`200 {"message":"denied tcp/80"}`. Refusals: `403 firewall_control_disabled`,
`400 firewall_port`, `400 firewall_comment`, `403 nft_permission`, `501
nft_unavailable`, `400 nft_refused`, `500 nft_failed`.

### `POST /api/firewall/delete`

```json
{ "handle": 4 }
```

`200 {"message":"deleted firewall rule 4"}`.

### `POST /api/firewall/flush`

No body. `200 {"message":"flushed 2 firewall rules"}`.

## Socket

```json
{ "type": "firewall_status" }
{ "type": "firewall_add", "data": { "action": "deny", "protocol": "tcp", "port": 80, "comment": "http" } }
{ "type": "firewall_delete", "data": { "handle": 4 } }
{ "type": "firewall_flush" }
```

Status answers `{"type":"firewall_snapshot","data":{...}}`; every mutation
answers `{"type":"firewall_changed","data":{"message":"..."}}`. The socket and
HTTP paths share the same `Daemon::execute` dispatch.

## CLI

```bash
anth-panel firewall status
anth-panel firewall list
anth-panel firewall deny 22
anth-panel firewall deny 53/udp --comment dns --yes
anth-panel firewall allow 80/tcp --yes
anth-panel firewall delete 4 --yes
anth-panel firewall flush --yes
```

`status` (alias `list`) prints HANDLE, ACTION, PROTO, PORT and COMMENT. A
missing table prints that it is not initialized. `deny` and `allow` take
`PORT[/PROTOCOL]`; the protocol defaults to `tcp`. A rule comment is optional.
Every mutation asks for confirmation on a TTY and refuses without `--yes` when
stdin is not a terminal. All mutations require `[firewall] control = true`.

## Web UI

**Network → Firewall** is the second tab. It reads on connect and refreshes
after every mutation; it does not hold an SSE stream.

- Toolbar: managed/not initialized/error state, rule count, Refresh, and a
  Flush button when control is on. Flush asks for confirmation.
- Context strip: `inet anthracite`, `chain input`, and `control`/`read-only`.
- Add form: action (Deny/Allow), port, protocol (tcp/udp), optional comment and
  Add rule. The hint explains that Allow removes a matching managed deny.
- One row per rule: handle, action badge, protocol/port, comment and Delete.
  Delete asks inline for confirmation. Custom rules show a neutral badge and
  `—` for the target.
- Missing table: an empty state explains that denying a port creates it.
- Raw ruleset: a collapsed `<details>` with the textual nft output.
- With control off, the add form, Flush and Delete are absent; the rules remain
  readable.
- A failed read or mutation keeps the last good list and shows the daemon
  message above it.
