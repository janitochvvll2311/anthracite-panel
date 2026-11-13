# Report — Service process hardening (L0)

## Summary

The first sandbox layer is in place: a service now starts with
`no_new_privs`, a parent-death signal, `dumpable=0`, a private umask and
per-class rlimits, and a refused launch is audited as a denial instead of
disappearing. The class is carried into the launch and the `service.start`
audit. This is **not** the full sandbox; it is the process baseline the later
filesystem, capability, namespace and cgroup layers build on.

## Delivered

- `ServiceLaunch` and `Service` carry `ServiceClass`; boot passes the manifest
  class and `service.start` records it.
- `spawn` applies, in `pre_exec` (allocation-free, so it stays safe after fork):
  - `PR_SET_NO_NEW_PRIVS`;
  - `PR_SET_PDEATHSIG(SIGKILL)`;
  - `PR_SET_DUMPABLE(0)`;
  - `umask(077)`;
  - `RLIMIT_CORE=0`, `RLIMIT_NOFILE=4096`;
  - `RLIMIT_NPROC=4096` for `exec`/`host`, `1024` for `socket`/`device`/`admin`.
- A failed `spawn` is audited as `service.deny` with `{"reason", "class"}`, the
  socket is removed and the launch is not retried; a refusal cannot be fixed by
  trying again.
- The `prctl` knobs are `#[cfg(target_os = "linux")]`; another Unix keeps the
  process group, the umask and the limits.
- The echo fixture exposes `/proc/self/status` fields; the lifecycle test
  asserts `NoNewPrivs: 1`.
- The design in `docs/design/28-service-sandbox.md` records L0–L5, the limits
  table, the class matrix, failure semantics, test plan and the Pulley decision.

## Pulley

**Decision: not shipped in v0.6.0.** Cranelift remains the only backend. Pulley
is an interpreter for W^X hosts and would need the same conformance, limits and
release testing without a deployment target asking for it. The runtime
scaffolding stays so it is a build flag later, not a redesign.

## Tests

- `engine/daemon` service tests: **10 passed**, including `NoNewPrivs=1` and
  `a_refused_launch_is_a_denial_not_a_crash`.
- `engine/daemon` boot tests: **6 passed**; services still start, answer, stop
  and restart with the limits.
- `cargo fmt --check` and `clippy -D warnings` on `anth-engine`.
- No real host namespace/PTY/mount validation in this layer.

## Out of scope

- Landlock or mount roots (L1).
- Namespaces per class (L2).
- Capability bounding/seccomp (L3).
- cgroups and disk quotas (L4).
- Manifest v4 `roots`/`observe`/`capabilities` declarations.

## Pending and risks

- **Services share the engine uid**, so L0 does not isolate them from each
  other or from the engine's own files. A per-service uid is the largest
  remaining gap and it is tracked in the design's open questions.
- `RLIMIT_NPROC` counts threads and is enforced per real user id, not per
  service. A value below the engine user's current thread count makes every
  `fork` inside a service fail, and the service then looks broken rather than
  denied. The chosen values are conservative.
- L0 limits and `no_new_privs` can break a service that legitimately needs a
  setuid helper; a real host must be tested before a release.
- `PR_SET_DUMPABLE(0)` makes `/proc/<pid>` root-owned, so a service cannot rely
  on reading another process's proc files; a debugger needs the same uid and a
  ptrace exception.
- The environment is cleared and `PATH` is fixed; a service that needs another
  variable must name it in the manifest/SDK later.
- L1 is required before the sandbox claim is honest.
