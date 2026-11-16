# Services

A **service** is a native process behind a private socket. It exists because a
wasm component cannot reach a PTY, `/proc`, sysfs, a mount or the network. The
public API is still WIT: a service exports interfaces like any provider, and the
engine calls it through `AppCaller`. There are no raw service routes.

A service is declared in a `manifest_version = 3` bundle ([Apps](apps.md)):

```toml
[service]
path = "bin/fs"
sha256 = "…"
class = "host"
argv = ["--socket", "{socket}", "--root", "/", "--trash", "{data}/trash"]
```

`class` is the authority the operator approves; `argv` is the exact command
line, frozen in the grant. The engine refuses a service whose binary hash, class
or argv changed without a new approval.

## Classes

| Class | The process may | Risk |
| --- | --- | --- |
| `exec` | execute programs and manage processes | host |
| `socket` | bind and use sockets | admin |
| `device` | use declared device nodes | admin |
| `admin` | configure the host (network, mounts, accounts) | admin |
| `host` | reach the host filesystem or kernel interfaces | host |

The class is the **declared** reach. It is shown at approval and frozen; it is
not a sandbox by itself ([Security](security.md#services-and-the-sandbox)).

## Placeholders

`argv` may name three directories, and the engine substitutes absolute paths:

| Placeholder | Path | Meaning |
| --- | --- | --- |
| `{socket}` | `$state/run/<app-id>/<pid>.sock` | the private socket the service listens on; exactly one |
| `{data}` | `$state/data/<app-id>` | the app's own `0700` directory; at most one |
| `{shared}` | `$state/shared` | the service trust domain; at most one |

`{socket}` is required in a service argv. A service receives no other path from
the engine, never sees `/api` and never gets the engine state. `{shared}` is
shared by every service that names it: a certificate published there by one
service is readable by another, and the grant is the boundary, not a per-file
ACL.

## Protocol

A service implements a tiny private protocol behind its socket:

```http
GET /health
POST /__anth/call
{"interface": "anthracite:fs/read@1.2.0", "function": "list", "input": {"path": "/"}}
```

`/health` is polled until `[limits] start-timeout-ms`; the reply is `200` with any
body. `/__anth/call` answers the function's result (or an error envelope); a
`stream`/WebSocket route uses `POST /__anth/stream`, which takes the same call
shape and answers with chunks. The
service's own WIT package is the schema; the `anth-service` SDK provides the
`Handler` trait and `serve(socket, handler)`, so a service author does not write
HTTP by hand.

The engine calls the service only through the grant: the interface must be
exported by the bundle and approved, and the class/argv must match. A call is
bounded by the app's limits and is audited with the caller, the callee and the
result.

## Calling another service: the broker

A service that imports an interface calls its provider through `{broker}`, not
through `/api/apps/...`:

```rust
let broker = anth_service::Broker::new("$state/broker/org.example.consumer.sock");
let answer = broker
    .call("anthracite:fs/read@1.2.0", "list", serde_json::json!({"path": "/"}))
    .await?;
broker.audit("proxy.apply", serde_json::json!({"host": "a"})).await?;
```

The engine owns the socket and checks the caller's grant: an interface the
manifest does not import is a denial, and every call is audited as
`broker.call` with the caller, the provider, the function and the outcome. The
provider's own limits apply. `anth:audit/record@1.0.0` is served on the same
socket and drops secret-shaped keys from the detail before writing it.

`GET /api/engine/composition` lists each app's `exports`, so a panel or a
manager can find the provider of an interface without reading a manifest.

## Supervision

At boot the supervisor:

1. creates the app's `{data}` (`0700`) and its socket directory;
2. runs the binary in its own session with a cleared environment, no stdio and a
   `SIGTERM`/`SIGKILL` process-group stop;
3. applies the process baseline: `no_new_privs`, `pdeathsig`, non-dumpable, umask
   `077`, a core limit of 0, `RLIMIT_NOFILE` 4096 and `RLIMIT_NPROC` 4096
   (`exec`/`host`) or 1024 otherwise;
4. waits for `/health` until the start timeout;
5. restarts a crash with exponential backoff, up to three restarts in sixty
   seconds; after the budget it reports the app as `degraded` and stops retrying.

Launch, refusal, crash and stop are audited as `service.start`,
`service.deny`, `service.crash` and `service.stop`. A refusal is a denial with
the class and the reason, never a crash, and it is not retried.

**The sandbox is L0 plus L1.** The child keeps the L0 hygiene and gains a
Landlock filesystem view built from the v4 `roots` plus the implicit
directories; there are no namespaces and no seccomp profile in 0.7. When the
engine runs as root, the service also gets a per-service uid and its `{data}`
and socket directory are owned by it, so the engine state and another app's data
stay unreadable even with a broad root. Without root, Landlock still applies but
the uid layer does not; approve broad roots accordingly.

A privileged listener (`< 1024`) works only when the engine holds
`CAP_NET_BIND_SERVICE` (root, or the capability in a container). Every other
capability is dropped; a declared listener without the capability is a
`service.deny` that names the layer. The network policy (`egress`) is still
intent until L2. See [§28](../design/28-service-sandbox.md).

## Third-party switch

`[services] allow-third-party` is a startup switch:

- A **release** build defaults it off. A third-party bundle that declares a
  service is refused at install unless the operator opts in.
- A **debug** build turns it on unless the file declares it off, so the host
  emulator (a third-party bundle) runs without a system key.
- A `system` bundle is unaffected: the distribution signature is the authority,
  not the switch.
- Turning the switch off and restarting stops third-party services; their
  consumers degrade without losing state.

## System-tier services

A service signed by the compiled distribution key installs as `system`, may use
the `anthracite` namespace, and skips the third-party switch. The operator still
sees the class and argv at approval; the difference is who signed the bytes
([Signing](signing.md)).

## Writing a service

```rust
use anth_service::{serve, Handler};
use anth_contract::Error;
use serde_json::{json, Value};

struct Files;

impl Handler for Files {
    fn call(&self, interface: &str, function: &str, input: Value) -> Result<Value, Error> {
        match (interface, function) {
            ("anthracite:fs/read@1.2.0", "list") => {
                Ok(json!([]))
            }
            _ => Err(Error::not_found(format!("no {interface}#{function}"))),
        }
    }
}

#[tokio::main]
async fn main() {
    let socket = std::env::args().nth(2).expect("--socket <path>");
    serve(&socket, std::sync::Arc::new(Files)).await.expect("serve");
}
```

Build the binary, hash it, put the hash in `[service] sha256`, package and sign
the bundle. The conformance kit runs the bundle offline; a third-party service
needs `--allow-third-party-service` there to match a daemon that opted in. Test
the process directly with a temporary socket: call `/health`, then
`/__anth/call`; a service that answers this pair is what the supervisor expects.

## Limits and failure behaviour

| Failure | Behaviour |
| --- | --- |
| health timeout | the app is degraded; the process is stopped |
| crash loop | three restarts in 60 s, then degraded |
| refused launch | `service.deny`, no retry |
| a call over budget | `exhausted`, the service stays up |
| missing binary or hash mismatch | refused at install/launch |

Service state belongs in `{data}`; the engine never deletes it on uninstall or
upgrade. A service that publishes files for another service uses `{shared}` and
documents that grant in its approval.
