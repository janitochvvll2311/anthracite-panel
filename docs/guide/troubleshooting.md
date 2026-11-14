# Troubleshooting

Start with `anth status`, `anth audit --limit 20` and the process log. Most
failures are one of the entries below.

| Symptom | Likely cause | Fix |
| --- | --- | --- |
| `cannot reach …engine.sock` | the engine is not running, or `ANTH_SOCKET`/`--socket` points elsewhere | start the panel/engine; pass `--socket`; check the socket's directory permissions |
| `403 password-change-required` | first boot, or the generated password was never changed | sign in at `/login`, change it at `/setup`; `--reset-credentials` mints a new one |
| `401 unauthenticated` on TCP | no session, expired (12 h by default), or a password change revoked it | log in again |
| `429 rate-limited` | five failed logins in a minute from one source | wait 60 seconds |
| `403 denied` with an origin message | `Origin` does not match `Host`; a proxy changed the host | use the public host, or have the loopback proxy set `X-Forwarded-Host` |
| `denied: no trusted key signs this bundle` | the signer is not in the keyring and is not the distribution key | `anth key add <public>` or build with the release key |
| `the signature does not verify` | the manifest/WIT changed after signing, or the wrong key | re-sign the exact `manifest.toml` + `wit/**`; check with `anth-conformance` |
| `this engine was built without a distribution key` | a keyless build read a stored `system` descriptor | use the release build, or reinstall the app as third-party |
| `the descriptor claims the system tier …` | a hand-edited `descriptor.toml` | reinstall the bundle; never edit the descriptor |
| `uses the first-party anthracite namespace` | a third-party bundle in a keyed build | sign it with the distribution key, or change the namespace |
| `invalid: apps.require-signature cannot be false in a release build` | a development config on a release binary | build a debug binary, or sign the bundles and keep the release rules |
| `the panel did not come up` | port in use, bad config, or missing frontend | read the log; `--listen` another port; `bun run build` in `panel/frontend` |
| release panel build fails on `frontend/dist` | the frontend was not built | `(cd panel/frontend && bun install && bun run build)` |
| `the service socket path is N bytes; the limit is 107` | the state path is too deep | move the state to a short path (`/var/lib/anth`, `/srv/a`) |
| a service is `degraded` | health timeout, crash loop, wrong binary/argv, or a missing dependency | read the panel log and `service.*` audit; check the binary hash and argv |
| an app is missing from the composition | not installed, not approved, not composed, or grant stale | install → approve → compose; `anth app show <id>` |
| `409 conflict` on install | the id is already installed | `anth app upgrade <id> <bundle>` |
| `409` with `dependents` | other composed apps depend on it | re-run with `--acknowledge dependents` (API: `X-Anth-Acknowledge: dependents`) |
| `412 precondition-failed` on compose | stale `If-Match` | re-read the composition and retry with its `ETag` |
| `503` on an app route | the app is not running (degraded, removed, not composed) | fix the composition and restart |
| `exhausted` | a stream or call hit its budget | raise the manifest `[limits]` or the app's usage |
| Turnstile: "not accepted" | wrong site key/secret, wrong hostname, wrong action, or an expired token | check `ANTHRACITE_CAPTCHA_*`; the widget must use `action=demo-session` |
| Turnstile: "temporarily blocked" | five failed challenges from the address in ten minutes | wait an hour, or test from another address |
| a public launcher refuses to start | no CAPTCHA, or no `ANTHRACITE_CAPTCHA_HOSTNAME` | set the Turnstile variables; bind loopback for local work |
| `demo-sessions` child port unreachable | the range is not published | `-p 8100-8199:8100-8199` (or the proxy maps it) and `ANTHRACITE_PORT_BASE` |
| Docker state is not writable | a bind mount owned by the host user | use a named volume, or `chown 1000:1000` (root for `base`) |
| Podman/Buildah: `cannot clone: Operation not permitted` | the build sandbox blocks user namespaces and cgroups are read-only | build images with `umoci`/`skopeo`; the archives are Docker-compatible |
| GitHub Actions jobs fail in ~2 s with `runner_id: 0` | no runner is assigned to the repository | run the local commands; it is an infrastructure issue, not a test failure |
| `anth doctor` cannot find `anth-engine` | the binary is not next to `anth` | set `ANTH_ENGINE=/path/to/anth-engine` |
| `key distribution` prints a `problem` | the binary was built with an unusable key | rebuild with the correct public key; every install is refused until then |
| audit has `problems` | a corrupt or truncated JSONL line | inspect `audit.jsonl`; keep the file, it is the record |

## Useful commands

```sh
anth status
anth audit --limit 50
anth app list
anth app show <id>
anth compose
anth key distribution
anth doctor --config /etc/anthracite/config.toml
```

For a bundle:

```sh
anth-conformance --json bundle.zip
anth-conformance --require-signature --key <public> bundle.zip
```

For the process: `journalctl -u anthracite` (or the log file), the panel's
`state/panel.log` in a release variant, and `GET /health` / `GET /api/health`.

## When it is not obvious

1. Reproduce with the CLI over the Unix socket; it removes the browser, session
   and proxy from the path.
2. Check the composition surface (`anth compose`) for `fatal` and `degraded`.
3. Read the last audit entries for the action that failed; a `deny` or
   `service.deny` names the layer and the reason.
4. Run the bundle through `anth-conformance`; it is the same verifier the engine
   uses.
5. If it still fails, collect the version (`anth --version`), the configuration
   (without secrets), the audit excerpt and the smallest reproduction.
