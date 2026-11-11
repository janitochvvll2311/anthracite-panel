# Accounts

Accounts is a native host service plus the sandboxed panel view. The demo
instance uses the shared scripted fixture from `demo/emulator`; this directory
builds the real app.

- `backend/` merges local `/etc/passwd`, `/etc/group`, `/etc/shadow` and
  `/etc/gshadow` with `getent passwd`/`getent group`, and reads the id ranges
  from `/etc/login.defs`. Every entry says `source = local` or `source = nss`;
  directory accounts are read-only. It never returns a password hash.
- `create-user`, `delete-user`, `set-lock`, `create-group`, `delete-group` and
  `set-password` run `useradd`, `userdel`, `usermod`, `groupadd`, `groupdel` and
  `chpasswd` by direct argv. A password only travels on `chpasswd`'s stdin; it
  never reaches argv or a log line. Password changes are not exposed as a CLI
  verb: the CLI would put the secret in its own argv, so use the Accounts view
  or the service route.
- The policy travels in the bundle argv, which is frozen in the grant and shown
  at approval:
  - `--control` enables user/group create, delete and lock; without it those
    mutations are refused and the inventory is read-only;
  - `--passwords` enables `set-password` independently of `--control`;
  - `--allow-system` permits system ids and the `system` flag;
  - `--allow-privileged-groups` permits `sudo`, `wheel`, `admin`, `root`,
    `docker` and `lxd` as primary groups;
  - `--home-root` bounds every home directory (default `/home`);
  - `--shell` adds a login shell to the allowlist; without it `/etc/shells` is
    used, then a small built-in set;
  - `--default-shell` is used when a create request omits the shell.
- Guards: uid/gid 0, the account running the service, the last member of any
  administrative group, system ids without the opt-in, privileged groups without
  the opt-in, homes outside `--home-root`, arbitrary login shells, and a group
  that is a user's primary group.
- Deleting a home is explicit (`remove-home`) and validated inside
  `--home-root`. A new user is locked until a password is set.
- If `/etc/shadow` is unreadable, locals report `locked = null`; the service
  never guesses. `groups` still works.

## Build

```bash
apps/accounts/build.sh
# writes apps/accounts/dist/accounts.anth
```

## Tests

```bash
cargo test --manifest-path apps/accounts/backend/Cargo.toml
cargo clippy --manifest-path apps/accounts/backend/Cargo.toml --all-targets -- -D warnings
```

The tests use fixture probes for `/etc` and `getent`, and a runner that records
argv without executing it; they never touch `/etc`. Real
`useradd`/`userdel`/`usermod`/`groupadd`/`groupdel`/`chpasswd` validation is
manual, on a disposable container with `shadow-utils`, and is not part of CI.
