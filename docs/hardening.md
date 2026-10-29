# Hardening notes

Phase 8 starts with property tests and a written security review. This is not a
claim that the panel is audited; it records what is enforced, what is tested and
what is still open.

## Property tests

The tests use `proptest` and arbitrary Unicode strings, including control
characters, NULs, separators and traversal spellings.

| Surface | Property |
| --- | --- |
| File paths (`files::resolve`, `split_parent`) | A resolved path always stays under the canonical file root; a split parent is relative and contains no `..`; nothing panics |
| Archive members (`archive::safe_member`) | An accepted relative path is not absolute and has no parent component, so joining it to the extract root cannot escape |
| Extension bundle members (`bundle::safe_member`) | Same confinement for extracted extension zips |
| Extension manifests (`Manifest::parse`) | Arbitrary text never panics; an accepted manifest has the pinned module path, ABI 3, a valid id and a 64-hex module digest |
| Config (`Config::from_toml`) | Arbitrary text never panics; parse errors stay errors |
| v0.2 migration (`migrate::plan`) | Arbitrary text never panics; an accepted plan validates and only carries an empty or 64-hex signing key |

The parser tests are deterministic and run in the normal `cargo test` pass, so a
regression is caught without a separate fuzz job. A future corpus-based fuzz
target can reuse the same entry points.

## Security review checklist

Enforced today:

- **Transport and auth.** HTTP mutations check `Origin` against `Host`; the
  socket checks peer credentials; sessions rotate and revoke; login failures are
  rate-limited. Secrets are `0600` and separate from `config.toml`.
- **Paths.** File and archive paths are lexically rejected before I/O and
  canonicalised afterwards; symlink escapes and `..` are refused. Archive
  extraction refuses absolute members, traversal and writes through
  archive-created symlinks.
- **Commands.** `ip`, `nft`, `mount`, `sfdisk`, `mkfs`, shadow-utils and extension
  capabilities are direct argv vectors; no shell. Values cannot start with `-`
  where the program would read them as flags.
- **Extensions.** Ed25519 over `manifest.toml`, per-file SHA-256, a frozen grant,
  archive member/size caps, a wasmi sandbox (fuel, memory, deadline, call and
  log budgets), grant-checked anchors, and an opaque-origin iframe with CSP and
  a narrow `postMessage` bridge.
- **Files.** Regular-file checks before opens, chunked reads/writes, size caps,
  no overwrite unless requested, and trash/archive member limits.
- **Processes.** Signals require `pid + start_ticks` and an allowlist; spawning is
  allowlisted and detached; pid 1, the daemon and other users are protected.
- **Accounts.** uid/gid 0, the daemon account, the last administrator, NSS
  entries, system ids and privileged groups are guarded.
- **Lifecycle.** Reboot is opt-in and capped; restart answers before replacing
  the process.

Still open / pending:

- Real `ip`/`nft` success paths need `CAP_NET_ADMIN` and `/dev/net/tun`.
- Real mount/partition/format success paths need `CAP_SYS_ADMIN`.
- WireGuard mutation jobs and private-key handling are not implemented; the
  reference extension is read-only.
- A corpus-based fuzz job for the archive and extension zip readers, plus a longer
  property budget in CI.
- An external review of the auth/session and extension trust boundaries before a
  `1.0.0` claim.
- Migration intentionally drops v0.2 sessions and extension grants.

## Running the checks

```bash
cd backend
cargo test --workspace
cargo test --workspace --release
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

The property tests are part of the library test target; no feature flag is
needed.
