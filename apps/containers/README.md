# Containers

A Docker-compatible container manager: inventory, lifecycle, create, logs,
stats, exec, images and named volumes, behind the runtime socket the operator
approves. Bundle `org.anthracite.containers`, package
`anthracite:containers@1.0.0`, service class `admin`, one `panel.view`, one FAB
and one `panel.modal`.

## Authority

```toml
[service]
class = "admin"
argv = ["--socket", "{socket}", "--data", "{data}", "--broker", "{broker}",
        "--runtime", "/var/run/docker.sock"]
roots = ["/var/run/docker.sock"]
```

- The runtime socket is a **literal argv entry** frozen in the grant; the
  service connects to it and never shells out to `docker`.
- That socket is **root-equivalent for containers**: a compromise of this
  service can control every workload the runtime owns. The sandbox and the audit
  bridge are the mitigations, not a claim that the socket is safe. A third-party
  bundle with this socket is not a supported deployment.
- The service never receives a host filesystem path through WIT. Named volumes
  are runtime objects; their host mountpoint is not rendered.
- Every mutation is audited with the container/image id and action. Environment
  values and registry credentials never reach an audit record or a log.
- A missing socket, a permission error and an unsupported API version are typed
  errors: the app starts and the view shows the error, the engine keeps running.
- When the engine is root, the service runs with a **dedicated uid, the engine
  gid and no supplementary groups** (the supervisor drops them before the uid
  change). An unprivileged engine runs the service as its own identity and
  keeps its groups; approval accepts a socket reachable through one of those
  groups. Docker installs its socket `root:docker 0660`; grant the engine gid
  before approval:

  ```sh
  chgrp <engine-gid> /var/run/docker.sock
  chmod 0660 /var/run/docker.sock
  ```

  Approval refuses a socket root the service could not connect to and names
  that command. It also checks search permission on every ancestor: a
  world-writable socket under a 0700 directory is refused, which is the
  rootless Podman socket seen from a root engine. A world-connectable socket
  also works, but is not recommended for a root-equivalent endpoint.

## Runtime

Docker Engine API v1.43+ over a Unix socket; Podman's compatible API, including
rootless, is the second target. The adapter speaks the HTTP API, not the CLI.
`--runtime-version` changes the prefix (`v1.43` by default) without changing the
bundle.

Create refuses what would escape the runtime's isolation:

- bind mounts (the WIT volume record only carries a named volume and an absolute
  container path; a source with `/` is refused);
- privileged mode and unknown fields (`deny_unknown_fields`);
- host and shared network namespaces (`host`, `container:*`);
- restart policies outside `no`, `always`, `unless-stopped`,
  `on-failure[:N]`;
- invalid ports, names, labels and environment variables.

Ports are `container`, `host:container` or `ip:host:container`, with an optional
`/tcp`, `/udp` or `/sctp`.

## Surface

| Route | Function |
| --- | --- |
| `GET /containers`, `GET /container?id=…` | inventory and details |
| `POST /containers` | create |
| `POST /container/start|stop|restart|pause|unpause|kill|remove` | one route per action |
| `GET /container/logs?id=…&tail=…&follow=…` | raw byte stream |
| `GET /container/stats?id=…` | sampled `sample` record stream |
| `POST /container/exec`, `GET /container/exec/attach`, `/exec-write`, `/exec-resize`, `/exec-close` | exec session |
| `GET /images`, `GET /images/pull`, `POST /images/remove` | images |
| `GET /volumes`, `POST /volumes/create`, `/volumes/remove` | named volumes |
| `GET /networks` | networks |

CLI: `anth app containers list|inspect|start|stop|images`.

The view has tabs for containers, images, volumes and networks. A container
card opens its details, lifecycle actions, a stats strip and a log pane; **Exec**
opens a shell dialog that attaches to the session, writes and closes it. The FAB
offers **Create container** and **Refresh**; the create modal has no bind or
privileged fields and refuses them in the service too. Destructive actions use
an in-app confirmation dialog, with an explicit force switch for a running
container.

Logs are raw bytes (Docker's 8-byte stream frames are decoded unless the
container is a TTY) and are budgeted by the engine, so a chatty container cannot
exhaust the panel. Stats are sampled every two seconds and streamed as records.

## Tests

- Fixture: `bun scripts/containers-fixture.ts [socket]` serves the Engine API
  used by the panel e2e (a running `web`, an exited `job`, one image, a volume
  and the bridge network). Build the QA bundle with
  `ANTH_CONTAINERS_RUNTIME=<socket> apps/containers/build.sh`, compose it, and
  run the spec with `ANTH_CONTAINERS_FIXTURE=1`; without that variable the spec
  skips.
- Unit: create validation (bind/privileged/host namespace, ports, names,
  volumes), the container/image/volume/network mappings, stats math, frame
  decoding and the port/reference helpers.
- Loopback (`cargo test --test loopback`): a fixture Engine API over a Unix
  socket drives the real service — list/inspect, create, start/stop/restart/
  pause/unpause/kill/remove, the bind and privileged refusals, logs, stats,
  exec attach/write/resize/close, pull, volumes, networks, and the audit shape
  (no environment value). Docker and rootless Podman remain manual QA rows.
- Frontend: `bun test` for the pure helpers; the panel e2e spec skips unless the
  composition carries the app.

## Decisions

- 0009 — the runtime socket is a literal argv root; the approval is
  root-equivalent.
- 0010 — exec sessions are served by the service over a hijacked stream, not by
  the engine; the terminal renderer consumes bytes.

## Limits

Kubernetes, Swarm, Compose, builds, registry auth, bind mounts, privileged
containers, host namespaces and device passthrough are out of scope. The view
does not stream pull progress beyond the last status line; the CLI/API exposes
the full stream.
