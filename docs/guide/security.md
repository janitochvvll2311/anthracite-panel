# Security

Anthracite's security model is the design of record; this page is the
operator-facing summary. The binding details are in [§9](../design/09-authority.md)
and [§28](../design/28-service-sandbox.md).

## The model in one page

- **The engine is the only authority.** Apps cannot grant themselves anything;
  the operator installs, approves and composes. The grant freezes the app id,
  version, manifest hash, signature, WIT set hash, exported and imported
  interfaces, risk, accepted levels, approver and time.
- **A bundle is signed and closed.** The manifest and `wit/` are covered by the
  signature; wasm, service and web bytes are covered by their hashes in the
  manifest. The engine verifies hashes and the signature before the app is
  visible, and validates the component's actual world against the manifest and
  the grant before it runs. A manifest that lies does not start.
- **Tiers come from the signer.** The distribution key (compiled in) earns
  `system`; the keyring earns `third-party`; the keyring can extend trust, never
  privilege. `anthracite` is first-party and a keyed build refuses it from a
  third-party bundle.
- **No paths to wasm.** A wasm app never receives a filesystem path or the
  network. Files, terminals, processes, network, storage and accounts are
  services with a declared class and argv, approved and audited.
- **Every state change is audited.** Install, upgrade, approve, revoke, enable,
  disable, remove, compose, key changes, session logins, restart, and every
  service lifecycle (including denials) land in the append-only audit log.

## Trust boundaries

| Boundary | What crosses it |
| --- | --- |
| operator → engine | the session/peer credential; approvals; configuration |
| engine → wasm app | only the granted interfaces, through the host catalog and the linker |
| engine → service | the private socket, the argv placeholders, the class; `{data}` and `{shared}` |
| service → service | `{shared}` is a shared directory; the grant is the boundary, not a per-file ACL |
| panel → app module | a sandboxed iframe with an opaque origin; only the app's own `/api/apps/{id}/` prefix |
| app → host | only what its service class declares, with the operator's approval |

## Services and the sandbox

The class is a **risk** level, not isolation. In 0.6 a service runs as the engine
user with the L0 baseline: its own session and process group, a cleared
environment, no stdio, `no_new_privs`, `pdeathsig`, non-dumpable, umask `077`,
core=0, `RLIMIT_NOFILE` 4096 and a process limit. It does **not** get a per-app
uid, namespaces, Landlock, seccomp or cgroups; those layers are proposed in
[§28](../design/28-service-sandbox.md) and are v0.7 work. A compromised `host`
service can reach what the engine user can.

What limits the damage today: the operator sees the class and argv at approval,
the third-party switch is off by default in a release, the service gets only the
paths in its argv, and the audit records every launch and denial. A deployment
that needs enforced filesystem or network isolation waits for the later layers;
do not describe the current model as a sandbox.

## Secrets

- The operator password lives in `$state/credentials.txt` (`0600`); sessions are
  in memory and a password change revokes them.
- Service secrets belong in the app's `{data}` (`0700`/`0600`), not in
  `anth:store/kv`. The panel may write a secret but never reads it back; no
  route, log or audit entry carries the material.
- The keyring stores public keys only. The distribution private key is never in
  the repository, the state tree or the build; see [Signing](signing.md).
- Audit `detail` carries identifiers and fingerprints, never PEM, tokens,
  headers or environment values.

## Exposure

- The Unix socket authenticates with the peer uid: the engine's own uid and root
  are accepted. Protect the socket's directory; a user who can write it can talk
  to the engine as that user.
- TCP is off by default. A TCP listener uses sessions: `HttpOnly;
  SameSite=Strict` cookies, an origin check on unsafe methods, login rate
  limiting, and CSP/`nosniff`/`no-referrer`/`X-Frame-Options: DENY` on every
  response. Put TLS in front of it; `--allow-remote` is an explicit decision.
- The panel is not an embeddable widget: `frame-ancestors 'none'`. The app frame
  is the only framed document, and it has an opaque origin.

## Known limitations

- **No CRL.** Revocation is uninstall, keyring removal, or a release-key
  rotation; an old binary keeps trusting the key it was built with.
- **No enforced service sandbox** beyond L0; the class is risk, and services share
  the engine user.
- **One operator.** There are no roles or per-user profiles; the audit names the
  actor, and the panel shares the operator session.
- **No per-user isolation in a shared state.** One engine state is one tenant;
  multi-tenant or per-browser isolation needs separate instances (the
  `demo-sessions` launcher is one example).
- **The engine trusts its host.** A compromised account that can replace the
  binary or the state is outside this model.

Report a vulnerability privately to the maintainer before public disclosure;
include the version, the configuration and the smallest reproduction.
