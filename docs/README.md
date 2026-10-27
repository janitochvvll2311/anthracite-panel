# Documentation

This branch is the v0.3.0 rewrite. Phases 0–3 and the terminal slice of Phase 4
are implemented; the remaining feature documentation is added with the code it
describes.

## User contract

| Document | Contents |
| --- | --- |
| [cli.md](cli.md) | Commands, flags, output, exit codes and bootstrap |
| [api-endpoints.md](api-endpoints.md) | HTTP routes, auth, files, terminal, SSE and error shape |
| [terminal.md](terminal.md) | PTY sessions, policy, socket/WebSocket contract and mobile UI |
| [socket-protocol.md](socket-protocol.md) | Framing, handshake, messages, errors and peer credentials |

## Design

| Document | Contents |
| --- | --- |
| [design/reference-v0.1-v0.2.md](design/reference-v0.1-v0.2.md) | What v0.1.0 and v0.2.0 built, what worked, and what failed |
| [design/v0.3.0.md](design/v0.3.0.md) | Architecture draft, decisions, target layout and phases |
| [design/phase-0-contracts.md](design/phase-0-contracts.md) | Phase 0 contracts: wire, config/state, bootstrap, HTTP/CLI, logging, git |
| [design/protocol-changelog.md](design/protocol-changelog.md) | Wire versions and the pre-1.0.0 compatibility scaffold |
