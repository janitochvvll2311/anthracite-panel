# Firewall

The daemon manages exactly one nftables table with two base chains:

```text
table inet anthracite
├── chain input  (type filter hook input priority filter; policy accept|drop)
└── chain output (type filter hook output priority filter; policy accept|drop)
```

Everything the panel reads or changes is scoped to that table. `nft` is invoked
directly with an argument vector, never through a shell, and the daemon never
touches another table.

The panel manages both `accept` and `drop` rules in both chains, plus each
chain's default policy. One managed rule exists per direction, protocol and
port: adding a rule for a target that is already managed replaces that rule, so
a target never has two conflicting verdicts.

An `accept` rule ends the current chain. It cannot override a `drop` in another
table, and a packet is accepted only if no matching rule or later chain drops
it. That is standard nftables behavior, not a panel limitation.

## Changing the default policy

Setting a chain policy to `deny` (nft `drop`) drops every packet that no rule
accepted and can lock you out of the panel and the host:

- `input` drop blocks new inbound connections, including the panel.
- `output` drop blocks outbound traffic, including the panel's responses.

The web UI and the CLI ask for confirmation before applying a policy. Keep an
out-of-band console available and add accept rules for loopback, established
traffic and your management port before switching a policy to `deny`.

## Configuration

```toml
[firewall]
control = false
```

`control` gates every mutation. Status is readable with it off, so an operator
can inspect the rules and policies without being able to change them. The
switch is reported in every snapshot. A change requires a daemon restart; the
running daemon keeps the policy it read at startup.

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
| `handle` | nftables rule handle, used by update and delete |
| `chain` | `input` or `output` |
| `action` | `allow`, `deny`, or `custom` for a rule the panel did not create |
| `protocol` | `tcp` or `udp`, absent for a custom rule |
| `port` | 1–65535, absent for a custom rule |
| `comment` | User comment for managed rules, or the raw nft comment for custom rules |

One chain state:

| Field | Meaning |
| --- | --- |
| `chain` | `input` or `output` |
| `initialized` | Whether the base chain exists in the managed table |
| `policy` | Effective default policy: `allow` (`accept`) or `deny` (`drop`) |

A snapshot also carries `initialized` (the table exists), `control_enabled`,
the textual `ruleset` for diagnostics, and `timestamp_ms`. A missing table is a
normal empty state, not an error: `initialized: false`, no rules, both chains
reported as not created with an `allow` policy, and an empty ruleset.

## Rule comments

Every managed rule carries:

```text
anth:<action>:<protocol>:<port>[|<comment>]
```

For example, `anth:deny:tcp:80|http`. The prefix is how the daemon recovers the
intent and the operator comment from nft itself; there is no side database. The
chain is not encoded in the comment: the rule already lives in `input` or
`output`.

A comment the daemon cannot parse becomes `custom` and is shown with its raw
text. Custom rules in the managed chains are still listed and can be deleted,
but not edited, because the panel cannot recover their match expression. Rules
in another chain are not managed and are ignored.

A user comment is optional, at most 32 characters, and limited to ASCII
letters, digits, `.`, `_` and `-`. This keeps the nft argument free of quoting
tricks; the daemon still passes it as one argv element.

## Mutations

All mutations require `[firewall] control` and answer with a one-line message.

### Add

```json
{ "chain": "input", "action": "deny", "protocol": "tcp", "port": 80, "comment": "http" }
```

`chain` defaults to `input` when omitted. Validates the port and comment,
creates the table and the requested chain if they are missing, then runs:

```text
nft add rule inet anthracite input tcp dport 80 drop comment "anth:deny:tcp:80|http"
```

`allow` adds an `accept` rule instead. If a managed rule for the same chain,
protocol and port already exists, it is replaced in place with `nft replace
rule ... handle N`.

### Update

```json
{
  "handle": 4,
  "chain": "output",
  "action": "allow",
  "protocol": "tcp",
  "port": 443,
  "comment": "https"
}
```

Rewrites one managed rule. When the chain does not change, the rule is replaced
in place and keeps its position. Changing the chain moves it: the new rule is
added to the target chain first, then the old one is deleted. Custom rules are
refused with `400 firewall_custom_rule`. A different rule already managing the
same chain/protocol/port is refused with `400 firewall_duplicate`. A zero
handle is `400 firewall_handle`; an unknown handle is `404
firewall_rule_not_found`.

### Delete

```json
{ "handle": 4 }
```

The daemon resolves the rule's chain from the current table and deletes that
handle from it. The handle is scoped to the managed chains, so a handle from
another chain cannot be reached. A zero handle is `400 firewall_handle`; an
unknown handle is `404 firewall_rule_not_found`.

### Flush

```json
{ "chain": "output" }
```

Removes every rule from the named chain, leaving the table, both chains and
their policies in place. `chain` defaults to `input`. The response counts the
rules that were present in that chain.

### Policy

```json
{ "chain": "input", "policy": "deny" }
```

Changes the default verdict of one base chain without touching its rules. The
daemon redefines the existing chain with `nft add chain inet anthracite input '{
policy drop; }'`, the documented nftables way to change a policy in place.
Setting the policy a chain already has is a no-op with a message.

## HTTP

All routes require a live panel session unless auth is disabled.

### `GET /api/firewall`

Returns the snapshot.

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
  "chains": [
    { "chain": "input", "initialized": true, "policy": "deny" },
    { "chain": "output", "initialized": true, "policy": "allow" }
  ],
  "ruleset": "table inet anthracite { ... }",
  "timestamp_ms": 1790000000000
}
```

### `POST /api/firewall/rule`

```json
{ "chain": "input", "action": "deny", "protocol": "tcp", "port": 80, "comment": "http" }
```

`200 {"message":"denied tcp/80 on input"}`.

### `POST /api/firewall/update`

```json
{ "handle": 4, "chain": "input", "action": "allow", "protocol": "tcp", "port": 443 }
```

`200 {"message":"updated firewall rule 4 to allow tcp/443 on input"}`.

### `POST /api/firewall/delete`

```json
{ "handle": 4 }
```

`200 {"message":"deleted firewall rule 4"}`.

### `POST /api/firewall/flush`

```json
{ "chain": "output" }
```

`200 {"message":"flushed 2 firewall rules from output"}`.

### `POST /api/firewall/policy`

```json
{ "chain": "input", "policy": "deny" }
```

`200 {"message":"set input policy to deny"}`.

Every rule mutation can refuse with `403 firewall_control_disabled`, `400
firewall_port`, `400 firewall_comment`, `403 nft_permission`, `501
nft_unavailable`, `400 nft_refused` or `500 nft_failed`.

## Socket

```json
{ "type": "firewall_status" }
{ "type": "firewall_add", "data": { "chain": "output", "action": "deny", "protocol": "tcp", "port": 80 } }
{ "type": "firewall_update", "data": { "handle": 4, "chain": "input", "action": "allow", "protocol": "tcp", "port": 443 } }
{ "type": "firewall_delete", "data": { "handle": 4 } }
{ "type": "firewall_flush", "data": { "chain": "output" } }
{ "type": "firewall_set_policy", "data": { "chain": "input", "policy": "deny" } }
```

Status answers `{"type":"firewall_snapshot","data":{...}}`; every mutation
answers `{"type":"firewall_changed","data":{"message":"..."}}`. `chain` defaults
to `input` when omitted on add, update and flush. The socket and HTTP paths
share the same `Daemon::execute` dispatch.

## CLI

```bash
anth-panel firewall status
anth-panel firewall list
anth-panel firewall deny 22 --yes
anth-panel firewall deny 53/udp --chain output --comment dns --yes
anth-panel firewall allow 80/tcp --yes
anth-panel firewall edit 4 443/tcp --action allow --comment https --yes
anth-panel firewall edit 4 --chain output --yes
anth-panel firewall policy input deny --yes
anth-panel firewall delete 4 --yes
anth-panel firewall flush --chain output --yes
```

`status` (alias `list`) prints each chain's policy and then HANDLE, CHAIN,
ACTION, PROTO, PORT and COMMENT. A missing table prints that it is not
initialized. `deny` and `allow` take `PORT[/PROTOCOL]`; the protocol defaults to
`tcp` and `--chain` defaults to `input`. `edit` changes only the fields given on
the command line: a new spec, `--chain`, `--action` and `--comment` are optional
and the current values are kept. An empty `--comment ''` clears it. `policy`
takes `CHAIN POLICY` where policy is `allow`/`accept` or `deny`/`drop`.

Every mutation asks for confirmation on a TTY and refuses without `--yes` when
stdin is not a terminal. A policy change to `deny` adds a lockout warning. All
mutations require `[firewall] control = true`.

## Troubleshooting

A `405 Method Not Allowed` on `/api/firewall/update` or `/api/firewall/policy`
means the HTTP daemon is older than the page: those paths fall through to the
static GET route. The same stale daemon ignores the `chain` field and adds every
rule to `input`, because its snapshot has no `chains` array. Rebuild and restart
the daemon with the current tree, then reload the page:

```bash
./scripts/dev.sh restart
```

For a release install, stage the shell and rebuild the binary first:

```bash
cd frontend
bun install
bun run build
./build-shell.sh
cd ../backend
cargo build --release
# then restart the service or process that runs anth-panel
```

`GET /api/firewall` is the quick check: a current daemon returns a `chains`
array with `input` and `output` policies. The startup line also names the wire
version, e.g. `anth-panel 0.3.0 (wire 3) listening on ...`; if the running
process does not print `wire 3`, it is still the old binary.

## Web UI

**Network → Firewall** is the second tab. It reads on connect and refreshes
after every mutation; it does not hold an SSE stream.

- Toolbar: managed/not initialized/error state, the active direction's rule count
  and Refresh.
- Context strip: `inet anthracite`, `input + output`, and `control`/`read-only`.
- Direction tabs: **Input** and **Output**, each with its rule count. They are a
  second tab level inside the Firewall tab, not another sidebar section. Only
  the active chain's policy, add form and rule list are shown.
- Active chain row: `managed`/`not created` state, current policy, a policy
  selector and Apply, and a Flush button. Applying a policy asks inline for
  confirmation; choosing deny warns that unmatched traffic will be dropped and
  can lock the operator out. Flush asks inline for confirmation.
- Add form: action (Deny/Allow), port, protocol (tcp/udp), optional comment and
  Add rule. The active tab supplies the direction. The hint explains that
  adding a rule for an existing protocol/port in that direction replaces it.
- One row per rule: handle, action badge, protocol/port, comment, Edit and
  Delete. Edit opens an inline form for direction, action, port, protocol and
  comment; Save sends an update and Cancel closes it. Moving a rule to the other
  direction makes it leave the current list after the refresh. Custom rules show
  a neutral badge, `—` for the target and no Edit button.
- Missing table: an empty state explains that adding a rule or changing a
  policy creates it.
- Raw ruleset: a collapsed `<details>` with the textual nft output for both
  chains.
- With control off, the add form, policy selectors, Apply, Flush, Edit and
  Delete are absent; the rules and policies remain readable.
- A failed read or mutation keeps the last good list and shows the daemon
  message above it.
