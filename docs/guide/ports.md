# Ports, roots and capabilities

A service declares what it needs in manifest v4; the operator approves that
declaration and the engine freezes it in the grant. This page is the operator
guide to that surface. Design: [§22](../design/22-platform-prerequisites.md),
[§28](../design/28-service-sandbox.md).

## Listeners

`listen` is a list of `tcp:<port>`, `tcp:<ip>:<port>`, `udp:<port>` or
`udp:<ip>:<port>`. IPv6 addresses use brackets (`tcp:[::1]:8080`). The port is
`1`–`65535`; ranges, wildcards, schemes and duplicates are refused. The engine
does not allocate or reserve ports: the service binds what it declared, and a
conflict is the service's startup failure, not a silent rebind.

The proxy declares `tcp:8080` and `tcp:8443`. Its document may map hosts onto
those two binds, but it cannot open a third listener the manifest did not
declare. A production proxy that must own `:80`/`:443` runs the engine as root
or with `CAP_NET_BIND_SERVICE`; the capability is granted from the declared
privileged listeners, not from `argv`.

## Roots

`roots` is a list of absolute paths, without `..`, with an explicit `:ro` or
`:rw`. `{data}`, `{shared}`, `{socket}` and `{broker}` are implicit; declaring
one again is an error. The sandbox applies Landlock to the declared roots plus
the implicit directories and the read-only resolver files
(`/etc/resolv.conf`, `/etc/hosts`, `/etc/nsswitch.conf`, `/etc/host.conf`,
`/etc/services`). A root that is a file keeps only file rights, which is how the
container manager holds a runtime socket. A root that no longer exists is
skipped and stays denied, so the service starts and reports a missing runtime
instead of failing to launch.

When the engine runs as root, each service gets a per-service uid and its
supplementary groups are dropped. `{data}` and the socket directory are `0700`
and owned by that uid; `{shared}` is `0770` with the engine gid. Without root,
Landlock still applies and the uid layer is unavailable; the release runs the
engine as root or with the equivalent capabilities.

## Egress

`egress` is `["any"]` or a list of `host:port` / `cidr:port`. It is **intent**
in v0.7.0: it is shown at approval and frozen, but network isolation (L2) is not
implemented, so a service that can connect is not stopped by the declaration.
The approval shows `egress_mode = intent` so the operator is not misled. The
proxy and ACME declare `any` because their peers come from operator
configuration; the store does too, for its sources.

## Approval text

At approval the panel shows, for every service:

- the **class** (`exec`, `socket`, `device`, `admin`, `host`) and the risk it
  implies;
- every **root** with its mode;
- every **listener** with its port and address, marked `enforced`;
- **egress** marked `intent`;
- for `admin`/`host`, the sentence that the class is declared reach, not a
  sandbox.

The five edge apps in one table:

| App | Class | Needs | The approval must say |
| --- | --- | --- | --- |
| `certs` | `admin` | scan roots, `{shared}/tls` | published keys are readable by peers granted `{shared}` |
| `acme` | `socket` | egress to the CA | the directory allowlist and `allow-private` opt-in decide the peer |
| `proxy` | `socket` | `tcp:8080`, `tcp:8443`, egress `any` | a document cannot invent a listener |
| `containers` | `admin` | the runtime socket (read/write) | the runtime socket is root-equivalent for containers |
| `store` | `socket` | egress `any`, `{data}` | downloading is not installing; the manager holds the session |

A launch the kernel refuses is audited as `service.deny` with the class and the
reason and is not retried.
