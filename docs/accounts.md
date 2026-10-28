# Accounts

The daemon lists the host's users and groups and can create or delete local
entries and set a password. Passwords are never returned: the inventory exposes
one derived bit, whether an account is locked.

Two sources answer two different questions:

- `/etc/passwd`, `/etc/group`, `/etc/shadow` and `/etc/gshadow` are the local
  records the panel can change.
- `getent passwd` and `getent group` are what the host resolves, which on a
  domain-joined machine is strictly more.

A local entry always wins when the two disagree: a directory account may not
silently shadow a local one. Every entry says where it came from, and an `nss`
entry is read-only.

## Configuration

```toml
[accounts]
control = false
manage_passwords = false
allow_system = false
allow_privileged_groups = false
home_root = "/home"
shells = []
default_shell = "/bin/sh"
```

- `control` gates user/group create and delete.
- `manage_passwords` gates password changes; it is independent of `control`.
- `allow_system` opts into accounts and groups below the host's `UID_MIN`/
  `GID_MIN`, including explicit ids and `--system`.
- `allow_privileged_groups` opts into `sudo`, `wheel`, `admin`, `root`, `docker`
  and `lxd`.
- `home_root` bounds every home directory the panel creates or removes.
- `shells` is the login-shell allowlist. Empty means `/etc/shells`, then a small
  built-in set; it never means "anything goes".
- `default_shell` is used when a create request omits the shell.

The snapshot reports these flags so the UI can hide an action the daemon would
refuse. It still validates every request against the current policy.

## Requirements

- `useradd`, `userdel`, `groupadd`, `groupdel` and `chpasswd` on `PATH`. A
  missing command is `501 account_unsupported`.
- Root (or the equivalent write access) in the daemon's process. Without it the
  tools fail with `403 account_permission`.
- `/etc/shadow` readable for the lock bit. If it is not, the snapshot degrades to
  `locked: null` with a warning; it never guesses "unlocked".
- `getent` for directory accounts. Without it the inventory lists local entries
  only and carries a warning.

## Data model

### User

| Field | Meaning |
| --- | --- |
| `name` | Login name |
| `uid`, `gid` | Numeric ids |
| `group` | Primary group name, or the numeric gid when it cannot be resolved |
| `gecos`, `home`, `shell` | Passwd fields |
| `locked` | `true`, `false`, or absent when the shadow entry cannot be read |
| `system` | `uid` is below the host's `UID_MIN` |
| `source` | `local` (writable) or `nss` (read-only) |
| `groups` | Supplementary group names; always present |

### Group

| Field | Meaning |
| --- | --- |
| `name`, `gid` | Group name and numeric id |
| `members` | Supplementary member names; always present |
| `system` | `gid` is below the host's `GID_MIN` |
| `source` | `local` or `nss` |

### Snapshot

```json
{
  "users": [
    {
      "name": "alice",
      "uid": 1001,
      "gid": 1001,
      "group": "alice",
      "gecos": "Alice Example",
      "home": "/home/alice",
      "shell": "/bin/bash",
      "locked": false,
      "system": false,
      "source": "local",
      "groups": ["wheel"]
    }
  ],
  "groups": [
    { "name": "wheel", "gid": 10, "members": ["alice"], "system": true, "source": "local" }
  ],
  "control_enabled": true,
  "password_enabled": true,
  "allow_system": false,
  "allow_privileged_groups": false,
  "timestamp_ms": 1
}
```

A malformed line is skipped and counted in `warning`; one damaged record does
not fail the inventory.

## Guards

- **Policy first.** `control` is checked before every create/delete;
  `manage_passwords` before a password change. `allow_system` and
  `allow_privileged_groups` are checked before the tool runs.
- **Local only.** An `nss` user or group is never modified: the panel could not
  write it back.
- **uid/gid 0.** Root is never modified through the panel, even with
  `allow_system`.
- **The daemon's own account.** It cannot be deleted; doing so would take the
  panel offline.
- **Last administrator.** Deleting the last account that is an administrator
  through `sudo`, `wheel` or `admin` is refused, whether the account is a
  supplementary member or has the administrative group as its primary group.
  Emptying an administrative group that is somebody's primary group is also
  refused.
- **System ids.** An existing entry below `UID_MIN`/`GID_MIN`, an explicit id in
  that range, or `--system` requires `allow_system`.
- **Privileged groups.** Creating or deleting `sudo`, `wheel`, `admin`, `root`,
  `docker` or `lxd` requires `allow_privileged_groups`. A group that is a
  user's primary group cannot be deleted.
- **Names.** 1–32 characters, starting with a lowercase letter or `_`, then
  lowercase letters, digits, `_` or `-`. This also stops a leading `-` from being
  read as a flag.
- **GECOS.** At most 256 characters, no `:`, newline or control characters.
- **Home.** Absolute, no `..`, and the resolved parent plus leaf must stay inside
  `home_root`. A home is removed only when the delete request asks for it.
- **Shell.** Must be on the effective allowlist.
- **Password.** 8–256 characters, no `:` or newline. The pair travels on
  `chpasswd`'s stdin; it never reaches argv or a log line.

New users are created locked (`useradd` without a password) and stay unable to
log in until a password is set. Creating a user and setting its password are
separate requests with separate switches.

## HTTP

`GET /api/accounts` requires a live panel session unless auth is disabled and
returns the snapshot. It is read-only and not rate-limited.

| Route | Body | Policy |
| --- | --- | --- |
| `POST /api/accounts/users` | `{"name":"alice","uid":1001,"gecos":"Alice","shell":"/bin/bash"}` | `[accounts] control` |
| `POST /api/accounts/users/delete` | `{"name":"alice","remove_home":false}` | `[accounts] control` |
| `POST /api/accounts/users/password` | `{"name":"alice","password":"..."}` | `[accounts] manage_passwords` |
| `POST /api/accounts/groups` | `{"name":"builders","gid":1500}` | `[accounts] control` |
| `POST /api/accounts/groups/delete` | `{"name":"builders"}` | `[accounts] control` |

A successful create or delete answers `200 {"message":"created user alice"}`.
Errors map to:

| Status | Code | Meaning |
| --- | --- | --- |
| 403 | `accounts_disabled` | `control` is off |
| 403 | `account_passwords_disabled` | `manage_passwords` is off |
| 403 | `account_protected` | uid/gid 0, the daemon account, the last administrator, a system id without opt-in or a privileged group without opt-in |
| 403 | `account_permission` | the tool could not write the account files |
| 404 | `account_not_found` | no such user or group |
| 409 | `account_conflict` | the name or id is taken, or a group is a primary group |
| 400 | `invalid_account` | bad name, id, home, shell, GECOS or password |
| 501 | `account_unsupported` | `nss` entry or missing shadow-utils tool |
| 500 | `account_failed` | the tool refused for another reason |

## Socket

```json
{ "type": "accounts" }
{ "type": "account_create", "data": { "name": "alice", "uid": 1001, "shell": "/bin/bash" } }
{ "type": "account_delete", "data": { "name": "alice", "remove_home": false } }
{ "type": "account_set_password", "data": { "name": "alice", "password": "..." } }
{ "type": "account_group_create", "data": { "name": "builders", "gid": 1500 } }
{ "type": "account_group_delete", "data": { "name": "builders" } }
```

`accounts` answers `{"type":"accounts","data":{...}}`. Mutations answer
`{"type":"account_changed","data":{"message":"..."}}`. Socket and HTTP share the
same `Daemon::execute` dispatch.

## CLI

```bash
anth-panel account list
anth-panel account ls
anth-panel accounts list
anth-panel account create alice --uid 1001 --gecos "Alice Example" --shell /bin/bash
anth-panel account delete alice --remove-home --yes
printf 'a long password' | anth-panel account password alice
anth-panel account group create builders --gid 1500
anth-panel account group delete builders --yes
```

`list` prints the policy flags, the users and the groups. `accounts` is an alias
for `account`. Create and delete require `[accounts] control`; password requires
`[accounts] manage_passwords`. Delete asks for confirmation on a TTY and refuses
without `--yes` when stdin is not a terminal. The password is read from stdin
and is never accepted as an argument; to change it interactively, use the web UI.

## Web UI

**Accounts** is one sidebar section with **Users** and **Groups** tabs, following
the Network section template:

- One feed, a status/count toolbar, refresh, a policy strip and a local filter.
- Users tab: a create form (name, optional uid/gid/GECOS/home/shell, system
  opt-in), then responsive cards with uid/gid, primary group, home, shell,
  supplementary groups, lock state and source.
- Groups tab: a create form (name, optional gid, system opt-in), then cards with
  gid, members and source.
- `Password` opens an inline form with confirmation; `Delete` opens an inline
  confirmation. A user delete offers "remove the home directory too".
- `nss` entries, uid/gid 0 and actions disabled by policy never show a button.
- Everything reaches 44 px on touch layouts; the cards stack at the component's
  own breakpoint.

## Validation

The full success path was exercised on 2026-10-28 against the development
container with shadow-utils installed: create a group, create a user (locked,
home created), set its password (`locked` became `false`), delete the user with
its home, delete the group, through both HTTP and CLI. The daemon ran as root
and `/etc/passwd`, `/etc/shadow` and `/etc/group` returned to their previous
contents. Network and storage success paths still need a container with
`CAP_NET_ADMIN` and `CAP_SYS_ADMIN`.
