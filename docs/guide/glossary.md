# Glossary

**App.** A signed bundle with a manifest, a WIT package and a wasm component, a
native service or both. Installed, approved and composed by the engine.

**Bundle.** The zip an app ships as: `manifest.toml`, `manifest.sig`, `wit/`,
`wasm/`, `bin/`, `web/`. Signature and hashes are verified before it is visible.

**Composition.** The document naming the profiles and the apps that should run.
It is a resource with an `ETag`; the engine resolves it into a surface.

**Conformance kit.** `anth-conformance`, which runs the engine's bundle verifier
offline: hashes, signature, WIT, world and service.

**Contribution.** A slot plus the app that provides it, as the panel surface
reports it. The panel decides how (and whether) to mount it.

**Distribution key.** The Ed25519 public key compiled into the release binary.
Its signature earns the `system` tier. The private half lives with the release
process, never in the repository.

**Engine.** The headless daemon (`anth-engine`) or the panel process that links
it: registry, verifier, resolver, supervisor, API. No UI, no feature logic.

**Fingerprint.** `sha256(public key)`, lowercase hex. It names a keyring entry and
is what the release notes publish.

**First boot.** The state's first start: the engine writes `credentials.txt` and
the setup gate requires a password change.

**Grant.** The frozen authority for an installed app: hashes, signature, WIT set,
interfaces, risk, accepted levels, approver and time.

**Interface id.** `namespace:package/interface@semver`, e.g.
`anthracite:fs/read@1.2.0`. The binding unit of the composition.

**Keyring.** The engine's trusted third-party signers, under `$state/keys`. It
extends trust, never privilege.

**Manifest.** The app's signed index: app facts, hashes, exports, imports,
routes, slots, CLI and limits. v2 is the original; v3 adds `[service]`.

**Namespace.** The package prefix of an interface id: `anth` (engine),
`anthracite` (first-party, `system` in a keyed build), anything else
(third-party).

**Panel.** The application that links the engine (or talks TCP), serves a UI and
mounts contributions. The official panel is one; the SDK is the contract.

**Peer credential.** The kernel-supplied uid on a Unix socket request. It is the
CLI's authentication; there is no token.

**Profile.** Debug or release, fixed by the binary. A release build refuses the
development switches.

**Quota.** The per-call budget of fuel, memory, wall time and stream bytes. A
hard budget ends with `exhausted`.

**Risk.** The computed levels an approval must accept: `read`, `write`, `admin`,
`host`, `package`. The import graph and a service's class add risk.

**Service.** A native process behind a private socket, exporting WIT interfaces.
Class `exec`, `socket`, `device`, `admin` or `host`; argv placeholders
`{socket}`, `{data}`, `{shared}`.

**Session.** A TCP bearer token in an `HttpOnly; SameSite=Strict` cookie. The
Unix socket does not use one.

**Signer.** The key that signed a bundle. The tier is derived from it, never from
anything the bundle declares.

**Slot.** A panel-defined UI position (`panel.view`, `panel.action`,
`panel.settings`, `panel.fab`, `panel.modal` are portable). The engine stores the
contribution; the panel renders it.

**Surface.** The resolved view of a composition: running apps, slots, CLI verbs,
links, degraded apps and a fatal error when the composition cannot resolve.

**Tier.** `engine`, `system` or `third-party`. `engine` is compiled in; `system`
needs the distribution key; everything else is `third-party`.

**WIT / world.** The WebAssembly Interface Types package under `wit/`. The app
declares exactly one `app` world; the engine validates the component's real world
against it and the grant.

**`$state`.** The engine's state directory (default `/var/lib/anth`): apps,
grants, keys, data, shared, run, composition, credentials, audit.
