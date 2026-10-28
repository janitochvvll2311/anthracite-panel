# Migration from v0.2

v0.3 splits policy from credentials. The `migrate` command maps the v0.2
settings file into a v0.3 `config.toml` and, when the account was present in the
old file, a `secrets.toml` with mode `0600`.

It is a dry run by default. Nothing is written until `--write` is passed, and an
existing target needs `--force`.

```bash
# Plan, print the resulting policy and the warnings
anth-panel --config /etc/anth/config.toml migrate --from /etc/anth/config.toml

# Apply in place
sudo anth-panel --config /etc/anth/config.toml \
  migrate --from /etc/anth/config.toml --write --force
```

## What is mapped

| v0.2 | v0.3 |
| --- | --- |
| `socket` | `socket_path` |
| `plugins` | `plugins.root` |
| `readable` | `files.root` |
| `terminal.enabled` | `terminal.enabled` |
| `auth.enabled` | `auth.enabled` |
| `auth.username` | `auth.username` |
| `auth.access_token_secs`, `auth.refresh_token_secs` | same names |
| `auth.password_hash` | `secrets.toml [auth] password_hash` |
| `auth.signing_key` | `secrets.toml [auth] signing_key` |

Everything else starts at the v0.3 release-safe default: feature switches off,
absolute paths under `/etc/anth` and `/var/lib/anth`, no plugin grant loaded.

## What is not migrated

- **Sessions.** The v0.3 session file has a different schema; old sessions are
  dropped and browsers sign in again. The signing key is preserved when present,
  so a new session can be issued without changing the account.
- **Plugin grants.** The manifest and grant formats changed. Reinstall the
  bundles and approve their capabilities again.
- **Unknown keys.** They are reported as warnings and ignored; the command never
  guesses at a v0.3 equivalent.

## Safety

- The v0.2 file is read first and never modified.
- The whole result is parsed and validated before the first write.
- Both files are written atomically; the secrets file is `0600`.
- A malformed v0.2 file, a non-hex signing key or an invalid result stops before
  writing anything.
- If the secrets write fails, the new config is removed again.

## Verified fixture

The test fixture covers a v0.2 file with custom paths, terminal, account,
signing key and an old session table. Tests assert the mapping, the warnings,
the dry run redacting the hash, the `0600` mode, the refusal without `--force`
and a malformed signing key stopping the plan. The command was also exercised
against a real file in `/tmp`.
