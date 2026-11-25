# 26. Container manager

> Part of the [design index](../../DESIGN.md). Scope for **v0.7.0 / M6**.
> Start conditions, prerequisites and sequencing: [§21](21-next-version-apps.md).

Bundle `org.anthracite.containers`, package `anthracite:containers@1.0.0`,
service class `admin`, one `panel.view`.

## 1. Scope

- Inventory containers, images, volumes and networks from a Docker-compatible
  runtime API (Docker Engine or Podman).
- Container details: image, command, state, health, ports, labels, created,
  restart policy, mounts (named volumes only), network.
- Lifecycle: start, stop with timeout, restart, pause/unpause, kill with an
  allowlisted signal, remove with a dependent check and confirmation.
- Create from an image: name, command, environment, published ports, named
  volumes, network, restart policy, labels. No bind mounts, no privileged mode,
  no host namespaces, no devices.
- Pull an image with progress; list and remove images with a dependent check.
- Logs: tail and follow, ANSI preserved, rendered by the terminal layer.
- Exec: a shell or command in the container, over the terminal renderer, with
  resize and close.
- Stats: CPU, memory, network and PIDs, sampled and streamed.

**Non-goals:** Kubernetes, Swarm, Compose stacks, image builds, a registry UI,
vulnerability scanning, bind mounts, privileged containers, host PID/network
namespaces, device passthrough, or rolling updates.

## 2. Authority

- Class `admin`; the runtime socket is a literal `--runtime` argv entry (for
  example `/var/run/docker.sock` or `$XDG_RUNTIME_DIR/podman/podman.sock`) shown
  at approval and frozen in the grant. The service connects to it; it never
  shells out to `docker`.
- The runtime socket is **root-equivalent** for containers. The approval text
  says so. A compromised service can control every workload the runtime owns;
  the service sandbox and the audit bridge are the mitigations, not a claim that
  the socket is safe.
- Every mutation is audited with the container/image id and the action, never the
  environment values.
- The service receives only the runtime socket path approved in argv; it does
  not receive filesystem paths through WIT or the engine. Named volumes are
  runtime objects; their host mountpoint is not rendered in the UI.

## 3. WIT outline

```wit
package anthracite:containers@1.0.0;

interface runtime {
  use anth:error/kinds@1.0.0.{error};

  record label { name: string, value: string }
  record env-var { name: string, value: string }
  record volume-mount { name: string, path: string }  // path is inside the container

  record container {
    id: string,
    name: string,
    image: string,
    state: string,
    status: string,
    health: option<string>,
    created-at: u64,
    ports: list<string>,
    labels: list<label>,
  }

  record image { id: string, tags: list<string>, size: u64, created-at: u64, digest: option<string> }
  record volume { name: string, driver: string, labels: list<label> }
  record network { name: string, driver: string, subnet: option<string>, containers: u32 }

  record create-request {
    image: string,
    name: option<string>,
    command: list<string>,
    env: list<env-var>,
    ports: list<string>,
    volumes: list<volume-mount>,
    network: option<string>,
    restart: string,
    labels: list<label>,
  }

  record sample { cpu-percent: f64, memory-bytes: u64, memory-limit: u64, net-rx: u64, net-tx: u64, pids: u64 }

  list-containers: func() -> list<container>;
  inspect: func(id: string) -> option<container>;
  create: func(request: create-request) -> result<container, error>;
  start: func(id: string) -> result<bool, error>;
  stop: func(id: string, timeout-s: u32) -> result<bool, error>;
  restart: func(id: string, timeout-s: u32) -> result<bool, error>;
  pause: func(id: string) -> result<bool, error>;
  unpause: func(id: string) -> result<bool, error>;
  kill: func(id: string, signal: string) -> result<bool, error>;
  remove: func(id: string, force: bool) -> result<bool, error>;
  logs: func(id: string, tail: u32, follow: bool) -> result<stream<list<u8>>, error>;
  stats: func(id: string) -> result<stream<sample>, error>;
  exec: func(id: string, command: list<string>, tty: bool, cols: u32, rows: u32) -> result<string, error>;
  exec-attach: func(session: string) -> result<stream<list<u8>>, error>;
  exec-write: func(session: string, data: list<u8>) -> result<bool, error>;
  exec-resize: func(session: string, cols: u32, rows: u32) -> result<bool, error>;
  exec-close: func(session: string) -> result<bool, error>;
  list-images: func() -> list<image>;
  pull: func(reference: string) -> result<stream<list<u8>>, error>;
  remove-image: func(id: string, force: bool) -> result<bool, error>;
  list-volumes: func() -> list<volume>;
  create-volume: func(name: string) -> result<volume, error>;
  remove-volume: func(name: string, force: bool) -> result<bool, error>;
  list-networks: func() -> list<network>;
}
```

Logs are raw bytes so ANSI survives; stdout and stderr are merged unless the
runtime exposes them separately. Exec reuses the terminal's session shape; the
panel's terminal SDK renders it, so the container app does not reinvent the key
bar or the resize logic.

## 4. Routes, CLI and UI

| Surface | Shape |
| --- | --- |
| `GET /containers` | `list-containers` |
| `GET /container?id=…` | `inspect` |
| `POST /containers` | `create` |
| `POST /container/start`, `/stop`, `/restart`, `/pause`, `/unpause`, `/kill`, `/remove` | one exact route per action; body carries the id and options |
| `GET /container/logs?id=…&tail=…&follow=…` | `stream = true`, `application/octet-stream` |
| `GET /container/stats?id=…` | `stream = true`, `events = true` |
| `POST /container/exec`, `/exec-write`, `/exec-resize`, `/exec-close` | one exact route per call |
| `GET /container/exec/attach?session=…` | `exec-attach`, `stream = true`, `application/octet-stream` |
| `GET /images`, `POST /images/pull`, `POST /images/remove` | list, pull stream, remove |
| `GET /volumes`, `POST /volumes/create`, `POST /volumes/remove` | list, create, remove |
| `GET /networks` | list |
| CLI `containers list`, `start`, `stop`, `logs`, `images` | the same calls |

The view is a container table with state and health, a details drawer, a log
pane, a stats strip and an exec tab. The FAB "Create container" opens a modal;
"Refresh" is the second action. Destructive actions use a confirmation modal
inside the app, not a panel modal.

## 5. Runtime compatibility

- Docker Engine API v1.43+ over a Unix socket; Podman's compatible API is the
  second target, including rootless. The adapter is internal: the service speaks
  the HTTP API, not the CLI.
- A missing socket, a permission error and an unsupported API version are
  distinct typed errors; the app degrades, the engine does not.
- Registry authentication is the runtime's business. The app does not store
  registry passwords in v0.7.0.

## 6. Dependencies and phases

| Phase | Delivers | Depends on |
| --- | --- | --- |
| K1 | Runtime client, inventory, details, stats read, logs tail | [§22 §2](22-platform-prerequisites.md#2-listener-and-egress-intent-in-the-manifest) |
| K2 | Lifecycle, remove guards, audit | K1, [§22 §7](22-platform-prerequisites.md#7-a-service-audit-bridge) |
| K3 | Create, pull, images, volumes, networks | K2, [§22 §4](22-platform-prerequisites.md#4-streams-quotas-and-backpressure) |
| K4 | Logs follow, stats stream, exec over the terminal renderer | K3, [§22 §4](22-platform-prerequisites.md#4-streams-quotas-and-backpressure) |
| K5 | Rootless Podman QA, sandbox, hardening | K4, [§22 §5](22-platform-prerequisites.md#5-system-tier-and-service-sandbox) |

## 7. Risks

- **Root-equivalent runtime socket.** The approval text is explicit; the service
  is system-tier and sandboxed in a release. A third-party service with this
  socket is not a supported deployment.
- **Image supply chain.** The app shows the image reference and digest; it does
  not build or scan. Pulling `latest` is allowed but warned.
- **Log volume.** Follow logs are budgeted; a chatty container cannot exhaust the
  panel.
- **Orphaned resources.** Removing a container leaves its named volumes unless
  the operator asks; the UI lists them.
- **Exec is a shell.** It runs inside the container, with the container's
  privileges. The container must not be privileged; v0.7.0 never creates one.

## 8. Implementation notes

The runtime socket is a file root (decision 0009): the Landlock rule keeps only
file rights for a non-directory, so the service can connect without exposing the
host. The WIT create record cannot express a bind mount, privileged mode or a
host namespace; unknown fields are rejected, and `network = "host"` or
`container:*` is refused explicitly. Exec is a service-owned session over the
hijacked Engine stream (decision 0010). The always-on test is a fixture Engine
API; Docker and rootless Podman are manual QA rows.

## 9. Acceptance

- The inventory works against Docker and rootless Podman.
- Start/stop/restart/remove work; a remove with dependent volumes asks first.
- Create rejects a bind mount, a privileged flag and a host namespace.
- Logs follow with ANSI and stop at the budget; stats stream at the configured
  interval.
- Exec opens, resizes, writes and closes through the terminal renderer.
- A missing runtime socket shows a clear degraded state and does not affect the
  other apps.

---
