# Containers

The containers app talks to a Docker-compatible Engine API: Docker Engine or
Podman. It lists containers, images, volumes and networks; it starts, stops,
restarts, pauses, kills and removes containers; it reads logs and stats and owns
exec sessions over the terminal layer.

Design: [§26](../design/26-container-manager.md). Decisions: [ADR 0009](../decisions/0009-how-does-the-container-manager-hold-the-runtime-socket.md),
[0010](../decisions/0010-how-do-exec-sessions-work-and-how-is-the-runtime-tested-without-.md).

## Approval

| Field | Value |
| --- | --- |
| App id | `org.anthracite.containers` |
| Service class | `admin` |
| Root | the runtime socket (`/var/run/docker.sock` by default), read/write |
| Interface | `anthracite:containers/runtime@1.0.0` |

The runtime socket is **root-equivalent** for containers. The approval text says
so: a compromised service can control every workload the runtime owns. The
socket path is a literal argv entry shown at approval and frozen in the grant;
the service connects to it and never shells out to `docker`. A missing socket is
a clear degraded state, not a boot failure.

## Behaviour

- Create from an image with name, command, environment, published ports, named
  volumes, network, restart policy and labels. **No bind mounts, no privileged
  mode, no host namespaces, no devices.**
- Start, stop with timeout, restart, pause/unpause, kill with an allowlisted
  signal, remove with a dependent check and confirmation.
- Logs tail and follow, ANSI preserved; stats sampled and streamed.
- Exec: the service creates the exec, starts it over a hijacked Engine stream
  and owns the session (read task, write half, resize and close). The engine
  only relays bytes.
- Every mutation is audited with the container/image id and the action; an
  environment value is never written to a log or an audit record.
- Removing the app makes the workload section disappear; no other app degrades.

## Routes and CLI

| Route | CLI | Function |
| --- | --- | --- |
| `GET /containers` | `anth list` | inventory |
| `GET /container` | `anth inspect` | one container |
| `POST /containers` | — | create |
| `POST /container/start` | `anth start` | start |
| `POST /container/stop` | `anth stop` | stop |
| `POST /container/{restart,pause,unpause,kill,remove}` | — | lifecycle |
| `GET /container/logs` | — | logs stream |
| `GET /container/stats` | — | stats stream |
| `POST /container/exec` + `GET /container/exec/attach` | — | exec session |
| `GET /images` | `anth images` | images |
| `GET /volumes`, `GET /networks` | — | runtime objects |

## Tests

`apps/containers/backend/tests/loopback.rs` runs a fixture Engine API over a Unix
socket: lifecycle, logs, stats, exec, refusal of mounts/privileged mode and the
audit shape. `tests/e2e/src/specs/containers.spec.ts` drives the same fixture
through the panel. Docker and rootless Podman are manual QA rows.
