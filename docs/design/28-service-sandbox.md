# 28. Service sandbox and Pulley

> Part of the [design index](../../DESIGN.md). Phase 9 of
> [§18](18-v0.6-plan.md). Status: **phase 1 implemented in v0.6.0**; the later
> layers are proposed.

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

- `PR_SET_NO_NEW_PRIVS`: no setuid/setgid/file capabilities through `exec`.
- `PR_SET_PDEATHSIG(SIGKILL)`: the child dies with the engine.
- `PR_SET_DUMPABLE(0)`: no core dump and no ptrace by an unrelated process.
- `umask(077)`: new files are private by default.
- `RLIMIT_CORE = 0`, a bounded `RLIMIT_NOFILE` and a per-class `RLIMIT_NPROC`.
- `setsid` as before; the supervisor keeps the process group.

L0 is not a complete sandbox. It removes the cheap escapes and gives the later
layers a stable baseline.

### L1 — filesystem (proposed)

- The manifest declares the roots a service may read or write:
  `roots = ["{data}", "{shared}", "/etc/ssl:ro"]` or an equivalent v4 field.
- Approval shows every root, its mode and the class.
- The supervisor applies **Landlock** (preferred, unprivileged) or a mount
  namespace with read-only binds when Landlock is unavailable.
- A service cannot see the engine state, another app's `{data}`, or a path it
  did not declare.
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

### L3 — capabilities and seccomp (proposed)

- Drop the capability bounding set to the class and the declared ports:
  `CAP_NET_BIND_SERVICE` for a privileged listener, `CAP_NET_ADMIN` for `admin`,
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

1. **L0 process hygiene** (this PR): no_new_privs, pdeathsig, dumpable, umask,
   per-class rlimits, audit class, fixture assertion.
2. **L1 roots**: manifest v4 roots, Landlock, approval display, negative tests.
3. **L3 capabilities/seccomp**: class policy, declared ports, seccomp allowlist.
4. **L2 namespaces**: per-class mount/PID/net/IPC policy.
5. **L4 cgroups/quotas**: memory, pids, CPU and disk.
6. **Pulley**: only if a target requires it; otherwise the decision stays.

## 9. Open questions

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
