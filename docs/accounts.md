# Accounts

The panel lists the host's users and groups and can change local ones. It is a
control-plane feature: every mutation runs as the daemon, through the system's
own account tools.

Control is off by default. Nothing here is reachable until it is turned on.

```toml
[accounts]
control = true
manage_passwords = false
```

## What it can read

Two sources answer two different questions.

- `/etc/passwd` and `/etc/group` are the files this panel can change.
- `getent passwd` and `getent group` are what the host actually resolves. On a
  machine joined to a directory service, that is more than the local files.

Each entry carries a `source`:

| `source` | Meaning |
| --- | --- |
| `local` | From `/etc/passwd` or `/etc/group`. Writable. |
| `nss` | Resolved by `getent` from somewhere else — LDAP, SSSD, systemd-homed. Read-only. |

A mutation on an `nss` entry is refused with `Unsupported` instead of being
attempted and failing halfway. That is deliberate: `useradd` would create a
second, shadowing local account with the same name, which is a confusing state
to recover from.

Passwords are never read. `/etc/shadow` is consulted for one bit per account,
locked or not, and a user whose lock state cannot be read reports `null`, which
the CLI shows as `?`. An unreadable shadow file is a normal unprivileged state,
not an error.

## What it can change

Every operation maps to one tool from `shadow-utils`:

| Operation | Tool |
| --- | --- |
| create user | `useradd` |
| change user | `usermod` |
| delete user | `userdel` |
| set password | `chpasswd` |
| create group | `groupadd` |
| change group | `groupmod` and/or `gpasswd` |
| delete group | `groupdel` |

The tools are not reimplemented by editing `/etc/passwd`. `useradd` takes the
lock, applies `login.defs`, creates the home directory from `skel` and
maintains `subuid`/`subgid`; a hand-rolled editor would get at least one of
those wrong. Where the tools are missing the answer is `Unsupported`, not a
guess built from different flags.

## Guards

These apply to every request and are not configurable.

- `uid 0` and `gid 0` are never modified, deleted or used as a new id.
- The daemon's own account cannot be deleted or locked. Demoting itself would
  take the panel offline mid-request.
- The last member of `sudo`, `wheel` or `admin` cannot be removed, and such an
  account cannot be deleted or locked. Root does not count as a fallback: on
  many hosts root is locked, and counting it would make the guard fail open.
- `nss` entries are read-only.
- A group that is somebody's primary group cannot be deleted.

These need an explicit opt-in:

- System accounts (below `UID_MIN` in `/etc/login.defs`) need
  `allow_system = true`.
- `sudo`, `wheel`, `admin`, `root`, `docker` and `lxd` need
  `allow_privileged_groups = true`. `docker` and `lxd` are on that list because
  a member can mount the host filesystem inside a container.

Input is validated as if it were going to a shell, because it is going to an
argv vector and a colon-separated file:

| Field | Rule |
| --- | --- |
| name | `^[a-z_][a-z0-9_-]{0,31}$`. The leading letter also stops `-rf` from being read as a flag. |
| gecos | No `:`, newline or control character. |
| home | Absolute, no `..`, and the resolved parent must be inside `home_root`. |
| shell | Must be on the allowlist. A login shell is a program that runs on login. |
| uid / gid | Refused when 0, or when already taken. |
| password | At least 8 characters, no `:` or newline. Sent to `chpasswd` on stdin, never on argv. |

A new account is created **locked**: the protocol has no password field on
create, so a new user cannot log in until a password is set separately.

## Configuration

| Key | Default | Meaning |
| --- | --- | --- |
| `control` | `false` | Enables every mutation below the read path. |
| `manage_passwords` | `false` | Enables `chpasswd`. Separate from `control`. |
| `allow_system` | `false` | Allows touching users and groups below `UID_MIN`/`GID_MIN`. |
| `allow_privileged_groups` | `false` | Allows touching root-equivalent groups. |
| `home_root` | `/home` | Home directories must resolve inside this. |
| `shells` | `[]` | Login shell allowlist. Empty means `/etc/shells`, then a small built-in set. |
| `default_shell` | `/bin/sh` | Shell for a user created without `--shell`. |

`control`, `manage_passwords`, `allow_system` and `allow_privileged_groups` are
off in release builds. Debug builds turn `control` on for local development, and
deliberately leave `manage_passwords` and both `allow_*` flags off, so a debug
build cannot change a password or escalate an account by accident.

`home_root` defaults to `/home`, so a create or an edit cannot place a home
outside `/home`. Loosening it to `/` turns the guard into a `..` and symlink
check rather than a boundary, which is worth doing only on a host whose homes
genuinely live elsewhere.

`allow_system` covers both account classes: a user below `UID_MIN` and a group
below `GID_MIN`. An explicit `--uid`/`--gid` into that range is refused the same
way `--system` is, because both land in system space. A group that is *also*
root-equivalent (`sudo`, `wheel`) reports the privileged warning first: it is
the one that explains the danger.

## Running the container

The image installs `shadow`, but runs as the unprivileged `anth` user. Account
tools need root to write `/etc/passwd`, `/etc/shadow` and `/etc/sudoers.d`, so
the default container answers `PermissionDenied` with an actionable detail.

Two supported deployments:

```bash
# 1. Run as root. The panel keeps its own guards, but the container is now root.
docker run --user 0 --cap-add NET_ADMIN ...

# 2. Keep USER anth and grant a narrow sudoers rule for the account tools.
#    /etc/sudoers.d/anth-accounts:
#    anth ALL=(root) NOPASSWD: /usr/sbin/useradd, /usr/sbin/usermod, \
#         /usr/sbin/userdel, /usr/sbin/chpasswd, /usr/sbin/groupadd, \
#         /usr/sbin/groupmod, /usr/sbin/groupdel, /usr/bin/gpasswd
```

The daemon tries `sudo -n` first when global `sudo = true` and falls back to the
direct call, so option 2 works with that setting on.

`passwd` from `shadow` has the setuid bit; `chpasswd` does not and does not need
it, because it is meant to be run by root.

## CLI

```bash
anth-panel account list
anth-panel account user create alice --comment "Alice Example" --shell /bin/sh
anth-panel account user modify alice --lock
anth-panel account user password alice          # prompts, no echo
passwd_string | anth-panel account user password alice --stdin
anth-panel account user delete alice --remove-home
anth-panel account group create developers
anth-panel account group modify developers --members alice,bob
anth-panel account group delete developers
```

`delete`, `password` and a membership change ask for confirmation unless
`--yes`. When stdin is not a terminal and `--yes` was not passed, they refuse
rather than assume.

The password is never accepted as a flag. It is read from `/dev/tty` with echo
off, or from stdin with `--stdin`, and is sent in the request body.

## API

| Route | Body |
| --- | --- |
| `GET /api/accounts` | — |
| `POST /api/accounts/users` | `{name, uid?, gid?, gecos?, home?, shell?, system?}` |
| `POST /api/accounts/users/{name}` | `{gecos?, shell?, home?, primary_gid?, locked?}` |
| `POST /api/accounts/users/{name}/delete` | `{remove_home?}` |
| `POST /api/accounts/users/{name}/password` | `{password}` |
| `POST /api/accounts/groups` | `{name, gid?, system?}` |
| `POST /api/accounts/groups/{name}` | `{gid?, members?}` |
| `POST /api/accounts/groups/{name}/delete` | — |

A mutation answers `{"message": "..."}`. Failures use the usual `{"error": "..."}`
shape with these status codes:

| Kind | Status |
| --- | --- |
| `invalid` | 400 |
| `disabled` | 403 |
| `protected` | 403 |
| `unsupported` | 501 |
| `not_found` | 404 |
| `conflict` | 409 |
| `permission_denied` | 403 |
| `failed` | 500 |

HTTP routes have no confirmation step; the frontend's dialog is the equivalent.
That is another reason `control` defaults to off.

## Risks

- **No authentication.** Anyone who can reach the HTTP port can use these routes
  when `control` is on. The terminal route has the same gap. Run the panel on a
  trusted network, or wait for auth before enabling control.
- **`manage_passwords` sends a plaintext password** over the socket and, on the
  HTTP path, over the network. TLS terminates in front of the panel or not at
  all; there is no transport security of its own.
- **A password reset is not audited.** Nothing records who changed what. The
  daemon logs the failure, not the success.
- **`home_root` is a path check, not a sandbox.** The default `/home` is a real
  boundary for a conventional host. Setting it to `/` keeps only the `..` and
  symlink check.
