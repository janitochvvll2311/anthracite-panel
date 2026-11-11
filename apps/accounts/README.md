# Accounts

Accounts is a native host service plus the sandboxed panel view. The demo
instance uses the shared scripted fixture from `demo/emulator`; this directory
builds the real app.

- `backend/` reads local `/etc/passwd`, `/etc/group`, `/etc/shadow` and
  `/etc/gshadow`, with the id ranges from `/etc/login.defs`. It reports users,
  groups and the derived lock bit; it never returns a password hash.
- `create-user`, `delete-user` and `set-lock` run `useradd`, `userdel` and
  `usermod` by direct argv. No shell is involved.
- The policy travels in the bundle argv, which is frozen in the grant and shown
  at approval:
  - `--control` enables create/delete/lock; without it the service is read-only;
  - `--allow-system` permits system ids and the `system` flag;
  - `--allow-privileged-groups` permits `sudo`, `wheel`, `admin`, `root`,
    `docker` and `lxd` as primary groups;
  - `--home-root` bounds every home directory (default `/home`);
  - `--shell` adds a login shell to the allowlist; without it `/etc/shells` is
    used, then a small built-in set;
  - `--default-shell` is reserved for the next contract phase.
- Guards: uid/gid 0, the account running the service, the last administrator,
  system ids without the opt-in, privileged groups without the opt-in, homes
  outside `--home-root` and arbitrary login shells.
- This phase lists **local** accounts only. Directory accounts resolved by NSS
  (`getent`) and the 1.2.0 contract (password changes, group CRUD, source,
  GECOS and home removal) are the next phase.
- If `/etc/shadow` is unreadable, `users` returns a typed `denied` instead of
  guessing the lock state; `groups` still works.

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

The tests use a fixture probe and a runner that records argv; they never touch
`/etc`. Real `useradd`/`userdel`/`usermod` validation is manual, on a disposable
container with `shadow-utils`, and is not part of CI.
