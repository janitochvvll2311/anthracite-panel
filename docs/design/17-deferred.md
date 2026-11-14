# 17. Deferred beyond v0.5.0

> Part of the [design index](../../DESIGN.md).

Everything this version consciously postponed, in one place. Each entry says what
it is, why it is not in v0.5.0, and what has to be true before it is worth taking
on. A deferred item is not a forgotten one: v0.5.0 is the platform, and the
platform is only finished when it can carry these.

---

## 1. Mechanisms

These are the four pieces of the contract that v0.5.0 describes but does not build.

### 1.1 Streams

`stream<T>` and `future<T>` in interfaces, with their HTTP mapping:
`stream<list<u8>>` as `application/octet-stream` (chunked) and `stream<record>` as
`text/event-stream`.

**Why deferred.** The toolchain works — a guest reads a host stream with
`block_on` inside a synchronous export — but the first real consumer is the
terminal, which is the hardest case: bidirectional, half-close, resize and
signals. Designing the shape against a toy guarantees redesigning it against a PTY.

**Ready when** the terminal app exists and the runtime can drive a store
concurrently for sources that block. Acceptance is not "it compiles": backpressure,
cancellation, half-close and error-after-partial-data each need a conformance test.

**Status: scheduled.** The terminal exists; the Phase 8 implementation plan is
[§20](20-streams.md).

### 1.2 Typed events

An event is a typed notification interface with fan-out fixed at compose time:

```wit
package anthracite:fs@1.0.0;

interface changed {
  record event { path: string, kind: enum { created, modified, removed } }
  on-change: func(event: event);
}
```

No topic strings, no payload schemas, no subscription grants.

**Why deferred.** Receiving one is easy; fanning out from a publisher to several
consumers needs either streams (§1.1) or generated glue over dynamically parsed WIT
types. Neither belongs in a milestone whose job is to prove routes and slots.

**Ready when** streams land and an app actually needs to push instead of being
polled.

### 1.3 Service apps

An app kind that runs as a supervised child process for kernel-facing work, with a
declared binary, an argv allowlist, a class (`exec`, `socket`, `device`, `admin`,
`host`) and a socket the supervisor hands over.

**Status: scheduled, not deferred.** v0.6.0 Phase 1 implements services as WIT
providers over a private `/__anth/call`, with manifest v3, the startup switch and
the host emulator before real host processes ([§19](19-v0.6-baseline.md)). The
earlier "raw HTTP" shape was dropped: a service is a transport behind the same
`AppCaller` as a component, so it stays inside the type system.

### 1.4 Blobs

`anth:store/blobs@1.0.0`, content-addressed, quota'd, over `stream<u8>`.

**Why deferred.** The resolver treats the native set as always available, so
declaring a capability the broker cannot answer turns a manifest error into a
runtime `503`. And with `list<u8>` it cannot carry a large file; the interface
would have to change contract as soon as streams arrive.

**Ready when** transfers land, together with their quota and their stream shape.

---

## 2. Applications

### 2.1 System apps

`fs`, `terminal`, `proc`, `net`, `storage`, `accounts` and `manager`, each as a
bundle with a manifest, each removable, none requiring an engine edit.

**Why deferred.** v0.5.0 stops at M3 by decision: the platform is judged by its
contract, not by its catalogue. The feature apps are the proof that the contract
holds, and that proof is done with `hello` and the panel first.

**Ready when** M0–M3 pass. They are judged against the safety invariants of
[§13](13-migration.md), not against v0.4.0's code.

### 2.2 The demo instance

A `config.demo.toml` with its own state, composing `compositions/demo.toml`, whose
apps export the real interfaces with generated data.

**Why deferred.** It is only meaningful once there are system apps to simulate.

**Ready when** §2.1 exists. Two constraints are already known: the generated data
must be deterministic, and per-browser isolation still waits on per-user profiles
(§3.4).

### 2.3 The panel web CLI

A panel view that runs the same verbs as `anth` — engine commands and the
`[[cli]]` verbs of installed apps — and renders the transcript in the browser. It
is an app, not a hidden panel feature: a service parses the verb and runs it under
the caller's grants, a `stream<list<u8>>` carries the output, and a slot module
renders it.

**Status: post-v0.6.0 backlog.** The CLI surface is still moving with the app
catalogue, and a web CLI needs a command runner that asks the engine instead of
shelling out to `anth` with ambient credentials. The Terminal already owns the
hard rendering and gesture work, so the first version should reuse that layer
rather than duplicate it.

**Ready when** the generated CLI verbs are stable and the Terminal's key bar and
gesture layer are available to other slot modules. Acceptance:

- runs engine and app verbs with the caller's grants and audit, with no shell
  escape by default;
- streams output through the same WebSocket/HTTP transport as the Terminal,
  including the HTTP fallback and byte budgets;
- offers the same gestures as the Terminal: mobile key bar, sticky Ctrl/Alt,
  special keys, symbols, Enter, font controls, double-tap Tab, pinch-to-zoom and
  press-and-drag arrows;
- keeps history, completion and copy/paste at the panel level, not inside each
  app.

### 2.4 The v0.7.0 business apps

The next milestone adds five apps that turn the panelled host into an operated
edge: a certificate manager, an ACME client, a reverse proxy, a container
manager and a provider-agnostic app store. They are not deferred from v0.6.0;
they are scheduled after it, because each depends on the service broker,
listener/egress approval and secret custody that v0.7.0 phase 0 adds, plus the
service sandbox that v0.6.0 completes.

**Why scheduled, not deferred.** They are the first apps whose value is measured
outside the host: TLS on the wire, workloads behind an ingress, and installs from
a signed catalogue. Building them before the v0.6.0 platform work would put
private keys in a wasm module or give an app the operator's session.

**Ready when** v0.6.0 is tagged and the prerequisites in
[§22](22-platform-prerequisites.md) pass. The scope, phases, removal matrix
and open decisions are [§21](21-next-version-apps.md).

---

## 3. Platform capabilities

### 3.1 Remote panel

A panel that runs in another process and talks HTTP instead of linking the engine.

**Why deferred.** Only the linked mode is built and tested; the remote one is kept
possible by design — explicit credentials, relative URLs, an origin-aware bridge,
a proxy that preserves method, body and headers.

**Ready when** someone needs a panel in another language or another machine. The
transport is already designed: HTTP with the same session model.

### 3.2 The Pulley profile

`anth-engine-pulley`, for hosts that forbid executable memory.

**Why deferred.** W^X hosts are a deployment profile, not the default, and the
scaffolding already makes it a build flag: one `Backend` enum, one crate boundary,
limits in fuel and memory, no precompiled artifacts in grants.

**Ready when** a deployment target requires it. Never as default.

### 3.3 Multi-operator

More than one operator, and what each may do.

**Why deferred (D8).** One operator is enough for an appliance, and roles without a
real deployment to shape them are guesswork.

**Ready when** a deployment asks. The audit already records `actor`, so the data to
decide is being collected.

### 3.4 Per-user profiles

Several compositions, or per-user scoping, inside one engine.

**Why deferred (D1).** The schema already has `[[profile]]`; what is missing is the
question of who a profile belongs to, which is an auth decision.

**Ready when** multi-operator lands. It also unblocks isolated demo state.

### 3.5 Revocation

A way to withdraw trust from a signer without uninstalling every app it signed.

**Why deferred (D6).** Key rotation in a release covers the distribution key;
anything finer needs a fleet that actually rotates.

**Ready when** a real deployment rotates keys, or a signer is compromised.

### 3.6 Per-app restart

Recompose by restarting one app instead of the engine.

**Why deferred (D2).** It is only safe when apps treat `anth:store` as their only
durable state, and that is a property of the apps, not of the engine.

**Ready when** the system apps prove they hold no state in memory across calls.

### 3.7 Rollback

Returning to the previous version of an app after an upgrade.

**Why deferred.** v0.5.0 documents "install the previous zip again". A real
rollback needs versioned content per app, which the store does not keep.

**Ready when** upgrades become frequent enough that reinstalling is a burden.

### 3.8 `nix` for syscalls

Adopt the `nix` crate for the POSIX calls the engine makes, keeping `libc` only
for what `nix` does not wrap (seccomp, landlock, syscalls newer than the crate).

**Why deferred (D-Syscalls).** The whole surface is four `unsafe` lines:
`geteuid` three times and one `access`. The `access` block is the only one with
real boilerplate (`CString`, errno), and `nix` would not remove `libc` from the
binary — it wraps it. Replacing eight auditable lines with a 0.x dependency that
also has to be audited is not a win at this size.

**Ready when** the service/PTY layer arrives and needs `prctl(PR_SET_PDEATHSIG)`,
`setsid`, process groups or signal handling, or when a third non-trivial call
appears. The threshold is deliberate: three or more calls, or the first one that
forces hand-rolled flags and errno. Adopt then with `default-features = false`
and only the features in use (`user`, `fs`, later `process`/`signal`).

**Detail to fix when touching it.** `access(2)` checks the **real** uid, while the
rest of the engine authorises with the effective uid. They are equal today because
the daemon is not setuid; `faccessat(AT_EACCESS)` states the right question and
should be used if the call is revisited for any other reason.

### 3.9 The distribution key and the system tier

A public key compiled into the binary that signs `system` apps, with the private
half held by the release process.

**Status: delivered as the mechanism.** v0.6.0 compiles the key in when
`ANTHRACITE_DISTRIBUTION_KEY` is set at build time, derives the tier from the
signer, validates a stored `system` descriptor against the key and reports the
fingerprint at `GET /api/engine/keys/distribution` (`anth key distribution`).
A build without the variable has no `system` tier, which is the honest debug
default. What is still owed to a deployment: the actual release key pair, the
step that signs the shipped bundles, and the rotation runbook
([§9.1](09-authority.md#91-tiers-and-signing)).

---

## 4. Developer experience

### 4.1 The conformance kit, published

`anth-conformance` as something a third party runs against their bundle offline,
with the same checks our CI runs.

**Status: delivered in v0.6.0.** `sdk/rust/conformance` builds
`anth-conformance`, which calls the engine's bundle verifier in a temporary
state tree. It checks layout, manifest, hashes, WIT, imports and signatures,
reports the app id/version/package/exports/imports/service class, and exits `1`
on a refusal.

### 4.2 Provider WIT for consumers

An app that imports `anthracite:fs/read` needs that package at build time. Today
the only path is vendoring a copy, which is the second truth we removed for the
host catalog.

v0.5.0 had no app-to-app imports; `hello` imports only host interfaces.

**Status: delivered in v0.6.0.** `anth app fetch-wit <id> [--out <dir>]` reads
the installed provider's WIT from `GET /api/engine/apps/{id}/wit` and writes it
under `wit/deps/<package>` by default. A registry and a version solver remain
later work.

### 4.3 `anth app inspect`

Parse a bundle locally and print what it would ask for, without a server.

**Why deferred.** Useful, but `app install` followed by `app show` already gives
the same information from the engine.

**Ready when** someone needs to inspect an untrusted bundle without installing it.

### 4.4 Audit and diff in the panel

Approval, risk and authority-diff screens in the panel's settings slot.

**Status: delivered in v0.6.0.** The Manager shows the frozen grant, the
effective risk and any newly required levels, and validates the checked levels
before approval. The diff is risk-level; an interface-level diff waits for the
surface to carry exports/imports.

---

## 5. Small enough to forget, listed so we do not

| Item | Note |
| --- | --- |
| `SlotContext` theme and locale | Today it carries `slot`, `app` and `api`. Both fields are additive, so no contract break |
| `panel.action` execution | The manifest gives a `target`; how the panel runs it (open a view, call a route) is unspecified |
| Product version vs crate version | Nothing relates `v0.5.0` to the `0.5.0` in `Cargo.toml` |
| `X-Anth-Acknowledge` tokens | Only `dependents` exists; the header is a list so others can be added |
| Route body limits | `apps.max-bundle-bytes` caps installs; per-route request bodies have no cap yet |

---

## 6. The gate

Nothing above starts before the acid test passes: an app written outside the engine
tree renders in a panel that was never compiled against it, and answers over HTTP
and the CLI. If that is not true, the deferred list is a distraction.
