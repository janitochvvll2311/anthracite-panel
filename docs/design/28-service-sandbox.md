# 28. Service sandbox and Pulley

> Part of the [design index](../../DESIGN.md). Phase 9 of
> [§18](18-v0.6-plan.md), then phase 0b of [§29](29-v0.7-plan.md). Status:
> **L0 implemented in v0.6.0**; **L1 and the port half of L3 implemented in
> v0.7.0** (D19); L2, the seccomp half of L3 and L4 remain proposed.

## 1. The problem

A service is a native process behind a private socket. It exists because a wasm
component cannot reach a PTY, `/proc`, sysfs, a mount or the network. That reach
is the feature and the risk.

Today the supervisor already:

- runs the child in its own session (`setsid`), with a cleared environment, no
  stdio and a `SIGTERM`/`SIGKILL` process-group stop;
- gives it a private socket under `state/run` (`0700`) and a private
  `state/data/<app-id>` (`0700`);
- checks the binary hash, class and argv at launch and freezes them in the grant;
- audits start, stop, crash, deny and restart budget.

But the child still runs as the **engine user**, with the engine's filesystem
view, capabilities and resource limits. `class` is a risk level, not a
sandbox ([§19 §4](19-v0.6-baseline.md#4-service-lifecycle-and-wit-rpc)). A
compromised `host` service can read anything the engine can read. That is why
third-party services remain behind `allow-third-party-service`, and why a
release cannot claim service isolation.

## 2. Threat model

The sandbox protects the engine, the panel and other apps from a **compromised
or malicious service**. It does not protect the host from the operator: a
service that is explicitly approved with `--root /` or the runtime socket is
trusted to use it.

The assets, in order:

1. The engine's state: credentials, grants, keys, other apps' data.
2. The engine's sockets: the control socket, the private service sockets.
3. Other app processes and their data.
4. The host outside the roots, ports and capabilities the grant approved.
5. Availability: a fork bomb, a disk fill or a fd leak must degrade one app,
   not the engine.

The sandbox is **fail-closed**: if a layer cannot be applied, the service does
not start, the app degrades, and `service.deny` is audited.

## 3. Layers

Each layer is independently testable and can land in its own PR.

### L0 — process hygiene (implemented)

| Knob | Value | Why |
| --- | --- | --- |
| `PR_SET_NO_NEW_PRIVS` | `1` | no setuid/setgid/file capabilities through `exec` |
| `PR_SET_PDEATHSIG` | `SIGKILL` | the child dies with the engine |
| `PR_SET_DUMPABLE` | `0` | no core dump, no ptrace by an unrelated process |
| `umask` | `077` | new files are private by default |
| `RLIMIT_CORE` | `0` | no core dump even if `dumpable` is reset |
| `RLIMIT_NOFILE` | `4096` | a fd leak degrades one app, not the host |
| `RLIMIT_NPROC` | `4096` (`exec`, `host`), `1024` (rest) | bounded fork bomb |
| `setsid` | as before | the supervisor keeps the process group |

The `prctl` calls are Linux-only. On another Unix the child keeps the process
group, the umask and the limits, and the missing layer is a documented gap.

Two properties of these values are easy to misread:

- **`RLIMIT_NPROC` counts threads, not just processes, and the kernel enforces it
  per real user id.** A service under its own uid gets its own budget; an
  unprivileged engine shares the limit with the engine and the other services.
  A value below the engine's own thread count makes every `fork` in a service
  fail, and the service looks broken instead of denied.
- **`PR_SET_DUMPABLE(0)` makes `/proc/<pid>` root-owned.** A service that reads
  another process's proc files, or a debugger attached by the same user, loses
  that access.
- **`umask(077)`** is defence in depth, not isolation: inside `{shared}` every
  service keeps the engine gid, so a `0600` file is still readable by a peer
  that runs under the same uid (the unprivileged case). With per-service uids it
  is one more layer.

L0 alone is not a complete sandbox: it removes the cheap escapes and gives the
later layers a stable baseline. L1 adds the filesystem view and, when the engine
is root, the per-app uid; isolation from a resource-hungry service still needs
the cgroup per service (L4).

A launch the kernel or the supervisor refuses is audited as `service.deny` with
the class and the reason, never as `service.crash`, and it is not retried: the
supervisor cannot fix a refusal by trying again.

### L1 — filesystem (v0.7.0)

- The manifest declares the roots a service may read or write:
  `roots = ["{data}", "{shared}", "/etc/ssl:ro"]` or an equivalent v4 field
  ([§29](29-v0.7-plan.md)).
- Approval shows every root, its mode and the class.
- The supervisor applies **Landlock** to every service: the declared roots plus
  the implicit directories (`{socket}`, `{data}`, `{shared}`, `{broker}`), the
  directory of the staged binary, and the read-only lookup files
  (`/etc/resolv.conf`, `/etc/hosts`, `/etc/nsswitch.conf`, `/etc/host.conf`,
  `/etc/services`). Name resolution is part of the network contract; without
  those files a service that resolves a host blocks until its caller times out.
  A root that is a file (the container runtime socket) keeps only the file
  rights; Landlock rejects directory rights on a non-directory. A root that no
  longer exists is skipped (its path stays denied) so a service can start and
  report a missing runtime instead of failing to launch. A kernel without
  Landlock refuses the launch.
- When the engine runs as root, each service gets a **per-service uid** derived
  from its app id and its supplementary groups are dropped; `{data}` and its socket directory are `0700` and owned by that
  uid, `{shared}` is `0770` with the engine gid, and the state root is `0711`
  (traversable, not listable). The verified binary is copied into the service's
  socket directory so the bundle tree stays unreadable.
- A service cannot see the engine state, another app's `{data}`, or a path it
  did not declare. Without root the uid layer is unavailable; Landlock still
  applies, and the release runs the engine as root or with the equivalent
  capabilities (D19).
- `{socket}`, `{data}` and `{shared}` are always implicit; a second declaration
  is an error.

### L2 — namespaces (proposed)

Per class, not one policy for all:

| Class | Mount | PID | Network | IPC/UTS | Notes |
| --- | --- | --- | --- | --- | --- |
| `exec` | private | private | shared | private | process execution; no host mounts |
| `socket` | private | private | shared | private | needs host network, not host mounts |
| `device` | private | private | shared | private | device nodes only if declared |
| `admin` | shared/controlled | private | shared | private | mounts and network config |
| `host` | shared | private | shared | private | host root; the `fs` case |

A private network namespace is **not** the default for `socket`: a proxy or an
ACME client needs the host network. The class says which reach is approved; the
sandbox narrows everything else.

### L3 — capabilities and seccomp (port half implemented in v0.7.0)

- **Implemented:** every capability is dropped from the bounding set except
  `CAP_NET_BIND_SERVICE`, which is installed as the only permitted, effective
  and ambient capability when a declared listener is below 1024. Without the
  capability in the engine, the launch is a `service.deny` naming the layer.
- **Proposed:** widen the class set to `CAP_NET_ADMIN` for `admin` and
  `CAP_SYS_ADMIN` only for `host`/`admin` where the argv needs a mount.
- A seccomp allowlist per class: no `ptrace`, no `bpf`, no `keyctl`, no
  `userfaultfd`, no module loading, no `kexec`, no namespace creation after
  startup unless the class needs it.
- `execve` remains allowed for `exec`/`host`; `socket`/`device` default to
  denying it unless a declared argv needs a helper.
- `no_new_privs` stays on.

### L4 — resource isolation (proposed)

- A cgroup v2 leaf per service, with memory, pids and CPU limits derived from
  the manifest budget.
- Disk quota for `{data}` and `{shared}` entries the service owns.
- A service that hits a limit is stopped and degraded; the engine stays up.
- The limits are shown at approval and frozen in the grant.

### L5 — observability (implemented in part)

- `service.start` carries the class; `service.deny` carries the layer and the
  reason.
- A sandbox refusal is a distinct audit reason, never a generic crash.
- The panel shows the degraded app and the reason.

## 4. Manifest and approval

L1 and L3 need declarations the v3 manifest does not have. The proposal is a
v4 `[service]` block with optional keys:

```toml
[service]
class = "host"
argv = ["--socket", "{socket}", "--root", "/"]
roots = ["/", "{data}", "{shared}"]
listen = ["tcp:8080"]
capabilities = ["CAP_NET_BIND_SERVICE"]
```

- v3 remains accepted; a v3 service gets L0 only and is explicitly marked
  `sandbox = "process"` in the surface.
- Every declared root/port/capability is shown at approval and frozen in the
  grant, like `argv` and `class`.
- The engine validates the shape; the supervisor enforces it.

## 5. Failure semantics

- A layer that cannot be applied refuses the launch: `service.deny` with
  `{"layer": "landlock" | "namespace" | "capability" | "seccomp" | "cgroup"}`.
- A refused service degrades its app and its consumers; the engine boots.
- A crash inside the sandbox still follows the existing restart budget.
- Stop remains `SIGTERM`, five seconds, `SIGKILL` on the process group.
- No sandbox layer may turn a service into a half-started process: the
  supervisor removes the socket and reaps the group on every failure.

## 6. Testing

- A fixture service reports `NoNewPrivs`, `Seccomp`, uid/gid and its limits via
  `/proc/self/status`, so L0 is asserted, not assumed.
- One fixture per class attempts the forbidden operations: read the engine
  state, open another app's data, bind an undeclared port, `ptrace` the engine,
  fork past the pid limit, load a module.
- Each negative test asserts a typed refusal, a `service.deny` entry and no
  engine crash.
- Real host validation is manual: a container with `CAP_NET_ADMIN` for `nft`
  and `ip`, a PTY, and a mount namespace; it is documented as manual because CI
  has no privileged host.
- The conformance kit stays bundle-only; runtime sandbox tests are the daemon's.

## 7. Pulley decision

**Decision: do not ship Pulley in v0.6.0.** The Cranelift build remains the only
backend. Pulley is an interpreter for W^X hosts; it is slower and would need the
same conformance suite, limit semantics and release testing as the default. No
deployment target in this milestone requires it, and shipping a second backend
without a user would be an untested promise.

The scaffolding stays: `anth-runtime` is the only crate that names wasmtime, the
`Backend` enum is the only selector, and limits are fuel/memory first. Revisit
when a real host forbids executable memory or a deployment asks for it.

## 8. Phases

1. **L0 process hygiene** (landed in v0.6.0): no_new_privs, pdeathsig,
   dumpable, umask, per-class rlimits, class in `service.start`, a `service.deny`
   on refusal, and a fixture assertion.
2. **L1 roots**: manifest v4 roots, Landlock, approval display, negative tests.
3. **L3 capabilities/seccomp**: class policy, declared ports, seccomp allowlist.
4. **L2 namespaces**: per-class mount/PID/net/IPC policy.
5. **L4 cgroups/quotas**: memory, pids, CPU and disk.
6. **Pulley**: only if a target requires it; otherwise the decision stays.

## 9. Open questions

- **Per-service uid.** Today every service runs as the engine user, so L0 cannot
  isolate services from each other or from the engine's own files. Giving each
  app a uid/gid needs `CAP_SETUID` or a root engine, and it changes `{data}`,
  `{shared}` and socket ownership. This is the single largest gap left by L0
  and it belongs to L1.
- Whether a v3 service should remain launchable in a release with L0 only, or
  require a migration to v4 before a release. The proposal is to keep v3
  launchable in debug and mark it `sandbox = "process"`; a release can refuse
  it with a deployment flag.
- Whether `roots` should use paths or capability names (`state`, `host`); paths
  are explicit and reviewable, names are portable.
- Whether `socket` services get a private network namespace by default; the
  proposal is no, because proxies need the host network.
- Whether cgroup v2 is available on every supported host; the fallback is
  rlimits and a documented limitation.
- Whether the lines between L0, L1 and L4 should be re-cut so the per-service
  uid lands with L0, since it is what makes the rest measurable.
