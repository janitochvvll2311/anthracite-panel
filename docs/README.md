# Documentation

This branch is the v0.3.0 rewrite. Phases 0–4 are implemented; Phase 5 is
code-complete with real-command validation pending, and Phase 6 has started with
the read-only Storage inventory. The remaining feature documentation is added
with the code it describes.

## User contract

| Document | Contents |
| --- | --- |
| [cli.md](cli.md) | Commands, flags, output, exit codes and bootstrap |
| [api-endpoints.md](api-endpoints.md) | HTTP routes, auth, files, terminal, processes, network, firewall, storage, SSE and error shape |
| [terminal.md](terminal.md) | PTY sessions, policy, socket/WebSocket contract and mobile UI |
| [processes.md](processes.md) | Process table, signals, spawn policy, SSE, CLI and the flat/tree UI |
| [network.md](network.md) | Interface inventory, addresses, counters, rates, default routes, guarded up/down/create/edit/rename/delete, SSE, CLI and the Interfaces tab |
| [firewall.md](firewall.md) | Managed nftables table with input/output chains, allow/deny/edit/delete/flush rules, default policies, guards, HTTP/socket/CLI and the Firewall tab |
| [storage.md](storage.md) | Read-only block devices, mounts, usage and swap over socket, HTTP, CLI and the Storage tabs |
| [socket-protocol.md](socket-protocol.md) | Framing, handshake, messages, errors and peer credentials |

## Design

| Document | Contents |
| --- | --- |
| [design/reference-v0.1-v0.2.md](design/reference-v0.1-v0.2.md) | What v0.1.0 and v0.2.0 built, what worked, and what failed |
| [design/ui-reference.md](design/ui-reference.md) | Approved Network-section UI as the template for new feature sections |
| [design/v0.3.0.md](design/v0.3.0.md) | Architecture draft, decisions, target layout and phases |
| [design/phase-0-contracts.md](design/phase-0-contracts.md) | Phase 0 contracts: wire, config/state, bootstrap, HTTP/CLI, logging, git |
| [design/protocol-changelog.md](design/protocol-changelog.md) | Wire versions and the pre-1.0.0 compatibility scaffold |
