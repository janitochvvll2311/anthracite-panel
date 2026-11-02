# 9. Authority

> Part of the [design index](../../DESIGN.md).

### 9.1 Tiers and signing

| Tier | Source | May request |
| --- | --- | --- |
| `engine` | compiled in | nothing; it *provides* the native set |
| `system` | signed by the distribution key, shipped in `apps/` | wasm, ui; services in v0.6.0 |
| `third-party` | any other signer, approved by the operator | wasm, ui only |

The **distribution key is compiled into the binary**. Rotating it requires a
release; no configuration file can mint a `system` app. An operator may add keys
to a keyring through `POST /api/engine/keys` (`anth key add <fingerprint>`), and
everything trusted that way is `third-party`: the keyring can extend trust, never
privilege.

An unsigned bundle is accepted only with `[apps] require_signature = false`, and
it is `third-party` regardless of its id — a manifest that claims `org.anth.*`
gains nothing.

Revocation is manual in v0.5.0: uninstall the app, or rotate the distribution key
in a release. There is no CRL, and that is a documented limitation, not an
oversight.

A third-party app never gets a service (`D-Services`). Services arrive in v0.6.0
with the terminal, and even then they stay `system`-tier: a service is a host
process, and "run arbitrary binaries" is not a capability we hand out with a
checkbox.

### 9.2 The closed native capability set

The engine provides exactly these interfaces, because they are mechanism-adjacent
and every app needs them:

```wit
anth:env/info@1.0.0      host facts (hostname, os, arch, uptime, cpu, memory)
anth:env/time@1.0.0      clock, monotonic and wall
anth:env/log@1.0.0       structured logging into the engine log
anth:env/random@1.0.0    cryptographically secure bytes
anth:store/kv@1.0.0      the app's own key/value namespace
```

Anything else — files, processes, terminals, network, storage, accounts, the app
manager itself — is an app that another app imports.

`anth:error` is not in this list because it is not a capability: it is a package of
types, injected when a package is parsed (§7.3), never granted and never risked.

`anth:store/blobs` is not in this set yet: it lands with transfers in v0.6.0.
Declaring it before implementing it would make the resolver promise a capability
the broker cannot answer.

### 9.3 Grants and approval

A grant freezes: app id, version, manifest hash, signature, the WIT set hash, each
exported interface, each imported interface — **not who provides it** — the risk
level, the approval levels the operator presented, who approved and when.

The provider is a composition fact, not a grant fact: it changes when the operator
edits the composition, without the app changing. Freezing it would either
invalidate every consumer's grant on a composition edit or leave it lying.

Grants scope **per interface**, which is why §3.6 insists on small interfaces. A
grant for `anthracite:fs/read@1.0.0` covers the functions that interface declares and
nothing else.

**Risk is a property of the graph, not a field of the manifest.** A manifest that
could declare its own risk could declare itself harmless while importing
`anthracite:fs/write`. The engine computes it instead:

1. Each host interface carries a fixed risk in the engine's catalog.
2. An app's risk is the maximum over the transitive closure of its resolved
   imports, plus the class of every service it declares (`nft` → `admin`, an
   arbitrary binary → `host`, a package install → `package`). In v0.5.0 no app can
   declare a service, so nobody reaches `admin` or `host`; the levels stay defined
   because the enum is part of the contract.
3. An app-owned interface contributes no risk of its own; it contributes what it
   can reach.
4. A manifest may declare intent — which service, with which argv class — but can
   never lower the computed result.

Because cycles are a composition error (§8.2), the closure is a finite DAG.

**When it is computed.** At approval, over the installed apps, so the operator sees
a meaningful number. At compose time, again, over the actual graph. If an app's
effective risk has grown past what was approved — because a provider was replaced
by one that reaches further, for example — the app is **blocked** and needs
re-approval (§8.5). A grant approves an authority; it cannot stay valid while the
authority it represents grows underneath it.

**One interface, one risk level.** An interface that both reads and writes would
force every grant to the higher level, so it is split instead (`anthracite:fs/read@1.0.0`,
`anthracite:fs/write@1.0.0`). That is what makes a read-only grant actually read-only.

| Level | Trigger (examples) |
| --- | --- |
| `read` | environment reads, clock, logs |
| `write` | mutating state, filesystem writes |
| `admin` | sockets, devices, network configuration |
| `host` | executing a binary, spawning a process |
| `package` | installing a system package |

Approval flags are derived from the risk of the whole set: `--accept-write`,
`--accept-admin`, `--accept-host`, `--accept-package`. `read` needs no flag: it is
the baseline, and requiring one for everything teaches operators to pass flags
without reading them. An operator who skips a level gets a refusal that names it.

### 9.4 What an app can never do

- call an interface it was not granted;
- see another app's state, cache or blobs;
- receive a filesystem path;
- reach the engine's socket, the panel's socket, or the network, unless a granted
  interface says so;
- mutate its own grant;
- change its exports without a new approval, because the WIT hash is in the grant.

### 9.5 Updates and re-approval

There is no auto-update. `anth app upgrade <bundle>` stages the new content, swaps
it atomically, and takes the app out of service until the question is settled:

- **Empty authority delta** — same interfaces, same providers, same limits, same
  tier — applies without re-approval. Hashes and signature are updated and the
  grant is rewritten around them.
- **Non-empty delta** requires approval. `anth app show --diff` prints it:
  interfaces added (with their computed risk) and removed, providers that
  changed, limits that grew, tier or signer changes.
- Approving requires the flags for the *new* risk. Removing a capability never
  requires re-approving the capability that went away.

A running app does not keep its old grant while an upgrade waits. It stops at
recompose and returns only approved. Half-applying an authority change is worse
than downtime.

The engine must still boot in that state, because approving is an engine
operation: an unapproved app degrades and `anth status` names it (§8.5). Otherwise
an upgrade would lock the operator out of the tool that approves it.

```bash
anth app upgrade ./fs.zip
anth restart                  # boots; fs degraded; its consumers 503
anth status                   # "org.anth.fs: unapproved"
anth app approve org.anth.fs --accept-write
anth restart
```

Removing an app, revoking its grant or disabling it while others depend on it
returns `409` and needs `--acknowledge dependents` (§6.4). The engine records the
acknowledgment with the dependents it broke.

Rollback in v0.5.0 is "install the previous zip again": documented, not
implemented.

---
