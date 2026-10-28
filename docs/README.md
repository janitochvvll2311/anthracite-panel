# Documentation

This branch is the v0.3.0 rewrite. Phases 0–4 are implemented; Phase 5 is
code-complete with real-command validation pending, and Phase 6 is code-complete
with Storage (inventory, guarded mount/unmount and partition/format) and
Accounts (inventory, guarded create/delete and passwords). Accounts success
paths were validated on a real host; storage mutations still wait for
`CAP_SYS_ADMIN`. Phase 7 is code-complete: signed bundles, frozen grants, the
plugin manager, the wasm ABI 3 runtime, guarded anchors, the sandboxed iframe
bridge and the `hello`/read-only WireGuard reference bundles. Phase 8
hardening/release is next. The remaining feature documentation is added with the
code it describes.

## User contract

| Document | Contents |
| --- | --- |
| [cli.md](cli.md) | Commands, flags, output, exit codes and bootstrap |
| [api-endpoints.md](api-endpoints.md) | HTTP routes, auth, files, terminal, processes, network, firewall, storage, accounts, plugins, SSE and error shape |
| [terminal.md](terminal.md) | PTY sessions, policy, socket/WebSocket contract and mobile UI |
| [processes.md](processes.md) | Process table, signals, spawn policy, SSE, CLI and the flat/tree UI |
| [network.md](network.md) | Interface inventory, addresses, counters, rates, default routes, guarded up/down/create/edit/rename/delete, SSE, CLI and the Interfaces tab |
| [firewall.md](firewall.md) | Managed nftables table with input/output chains, allow/deny/edit/delete/flush rules, default policies, guards, HTTP/socket/CLI and the Firewall tab |
| [storage.md](storage.md) | Block devices, mounts, usage, swap, guarded mount/unmount and partition/format over socket, HTTP, CLI and the Storage tabs |
| [accounts.md](accounts.md) | Users and groups from local files plus NSS, lock state, guarded create/delete, password changes and the Accounts tabs |
| [plugins.md](plugins.md) | Signed bundles, manifests, frozen grants, install/list/enable/remove, the ABI 3 runtime, anchors, the iframe sandbox, reference plugins and the Settings → Plugins panel |
| [migration.md](migration.md) | Dry-run `migrate` from v0.2: policy mapping, secrets split, dropped sessions/grants and safety rules |
| [hardening.md](hardening.md) | Property-test coverage, enforced security boundaries and the open hardening items |
| [release.md](release.md) | Tarball layout, systemd unit, container build, checksum and the pre-release suffix policy |
| [demo.md](demo.md) | In-progress hosted demo, `ANTH_BACKEND=demo`, simulated network/firewall and the host boundary |
| [socket-protocol.md](socket-protocol.md) | Framing, handshake, messages, errors and peer credentials |

## Design

| Document | Contents |
| --- | --- |
| [design/reference-v0.1-v0.2.md](design/reference-v0.1-v0.2.md) | What v0.1.0 and v0.2.0 built, what worked, and what failed |
| [design/ui-reference.md](design/ui-reference.md) | Approved Network-section UI as the template for new feature sections |
| [design/v0.3.0.md](design/v0.3.0.md) | Architecture draft, decisions, target layout and phases |
| [design/phase-0-contracts.md](design/phase-0-contracts.md) | Phase 0 contracts: wire, config/state, bootstrap, HTTP/CLI, logging, git |
| [design/protocol-changelog.md](design/protocol-changelog.md) | Wire versions and the pre-1.0.0 compatibility scaffold |
| [design/cli-http-parity.md](design/cli-http-parity.md) | Audit matrix of socket, HTTP and CLI coverage, plus the transport-only exceptions |
