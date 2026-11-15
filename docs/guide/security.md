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

The class is a **risk** level; the sandbox is separate. A service keeps the L0
baseline (own session and process group, cleared environment, no stdio,
`no_new_privs`, `pdeathsig`, non-dumpable, umask `077`, core=0,
`RLIMIT_NOFILE` 4096 and a process limit) and gains **L1**: a Landlock view over
its declared roots plus the implicit directories, and, when the engine runs as
root, a per-service uid owning its `{data}` and socket directory. It still does
**not** get namespaces, seccomp or cgroups; those remain proposed in
[§28](../design/28-service-sandbox.md).

What limits the damage: the operator sees the roots, listeners and class at
approval; egress is still intent until L2; the third-party switch is off by
default in a release; the service gets only the paths in its grant; and the
audit records every launch, denial and refusal layer. A deployment that needs
enforced network isolation waits for L2.

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
- **No namespaces, seccomp or cgroups**; L1 confines the filesystem and, under
  root, the uid. Egress is intent until L2, and a service with a broad root is
  as broad as the operator approved.
- **One operator.** There are no roles or per-user profiles; the audit names the
  actor, and the panel shares the operator session.
- **No per-user isolation in a shared state.** One engine state is one tenant;
  multi-tenant or per-browser isolation needs separate instances (the
  `demo-sessions` launcher is one example).
- **The engine trusts its host.** A compromised account that can replace the
  binary or the state is outside this model.

Report a vulnerability privately to the maintainer before public disclosure;
include the version, the configuration and the smallest reproduction.
