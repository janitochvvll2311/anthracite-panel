# Report — Real accounts service

## Summary

The emulator fixture was replaced by a native accounts service for
`anthracite:accounts`. The work covers the v0.4 checklist in two phases:
inventory and basic mutations on 1.1.0, then full parity on 1.2.0 with
`getent`/`source`, passwords, groups, GECOS and optional home removal.

## Integrated phases

| Phase | PR | State | Merge |
|-------|----|-------|-------|
| Real accounts service (1.1.0) | #150 | merged | `29effd7` |
| Accounts 1.2.0 parity | #151 | merged | `4578671` |

## Delivered features

- Native `accounts-service` with class `host`, a probe for `/etc` and
  `getent`, and an injectable command runner.
- Local and NSS inventory: `source` local/nss, optional lock state, GECOS,
  primary group and supplementary groups.
- `create-user`, `delete-user` (with `remove-home`), `set-lock`,
  `set-password`, `create-group` and `delete-group`.
- Mutations through argv (`useradd`, `userdel`, `usermod`, `groupadd`,
  `groupdel`), never a shell; passwords only on `chpasswd` stdin.
- Policy through argv: `--control`, `--passwords`, `--allow-system`,
  `--allow-privileged-groups`, `--home-root`, `--shell`, `--default-shell`.
- Guards for root, the service account, the last member of each administrative
  group, system ids, privileged groups, home, shell and primary group.
- Real bundle, manifest and build script; the demo emulator implements the same
  contract.
- Accounts view: policy strip, local/directory/lock badges, user and group
  forms, password changes, delete with or without the home, and visibility
  refresh.
- `getent` with a 2 s timeout and a reader thread for large directories or
  non-UTF-8 bytes.

## Review findings addressed

- Phase 1: per-group last-administrator guard, real unlock verification, false
  conflict for a same-name group, and serialized mutations with a mutex.
- Phase 2: `home-root`/`default-shell` defaults in the view and backend,
  `not_self` in `set-password`, removal of the CLI verb that exposed the
  password, root/operator protection in the fixture, a `remove-home` test,
  `getent` timeout/draining, and the Password button disabled for protected
  accounts.

## Tests

- `apps/accounts/backend`: **37 tests**, `clippy -D warnings` and
  `fmt --check` clean.
- `demo/emulator`: **44 tests**, clippy and fmt clean.
- `apps/accounts/frontend`: `tsc --noEmit` and `bun build` clean.
- Demo e2e through the engine: user creation with automatic ids, password,
  groups, delete with home, and the Files subtree check.
- Real smoke test of the 1.2.0 bundle against `/etc`, with engine validation of
  `policy`, `users` and `groups`.
- Real mutation validation does not run in CI: it is manual in a disposable
  container with `shadow-utils`.

## Recorded decisions

- Local accounts first; `getent` and `source` in the second phase.
- Multi-user identity and per-user roles are deferred to a platform feature,
  `identity & permissions`, in a later version.
- Accounts policy travels in the frozen grant argv, not in app-editable
  configuration.
- A `getent` failure or timeout degrades to the local inventory and never blocks
  the service.
- The password is not exposed as a CLI verb because the CLI would place it in
  argv.

## Out of scope

- Multi-user identity, roles and per-user permissions.
- Manager, audit and approval UI (M4-close/M5).
- Pagination or filtering for large inventories; the list returns the whole
  snapshot.
- Automated validation of real host mutations.

## Pending and risks

- Manually validate user create/delete, password, groups and home removal in a
  disposable container with `shadow-utils`.
- `getent` can still add latency on hosts with very large directories; consider
  caching or pagination in a later phase.
- The service needs root or equivalent access to mutate; CI only exercises
  fixtures.
