# Report — Service process hardening (L0)

## Summary

The first sandbox layer is in place: a service now starts with
`no_new_privs`, a parent-death signal, `dumpable=0`, a private umask and
per-class rlimits. The class is carried into the launch and the `service.start`
audit. This is **not** the full sandbox; it is the process baseline the later
filesystem, capability, namespace and cgroup layers build on.

## Delivered

- `ServiceLaunch` and `Service` carry `ServiceClass`; boot passes the manifest
  class and `service.start` records it.
- `spawn` applies, in `pre_exec`:
  - `PR_SET_NO_NEW_PRIVS`;
  - `PR_SET_PDEATHSIG(SIGKILL)`;
  - `PR_SET_DUMPABLE(0)`;
  - `umask(077)`;
  - `RLIMIT_CORE=0`, `RLIMIT_NOFILE=4096`;
  - `RLIMIT_NPROC=4096` for `exec`/`host`, `1024` for `socket`/`device`/`admin`.
- The echo fixture exposes `/proc/self/status` fields; the lifecycle test
  asserts `NoNewPrivs: 1`.
- The design in `docs/design/28-service-sandbox.md` records L0–L5, the class
  matrix, failure semantics, test plan and the Pulley decision.

## Pulley

**Decision: not shipped in v0.6.0.** Cranelift remains the only backend. Pulley
is an interpreter for W^X hosts and would need the same conformance, limits and
release testing without a deployment target asking for it. The runtime
scaffolding stays so it is a build flag later, not a redesign.

## Tests

- `engine/daemon` service tests: **9 passed**, including the new
  `NoNewPrivs=1` assertion.
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

- L0 limits and `no_new_privs` can break a service that legitimately needs a
  setuid helper; a real host must be tested before a release.
- `RLIMIT_NPROC` is per real user, not per service; a lower value than the
  engine user's process count would fail the launch. The chosen values are
  conservative.
- The environment is cleared and `PATH` is fixed; a service that needs another
  variable must name it in the manifest/SDK later.
- L1 is required before the sandbox claim is honest.
