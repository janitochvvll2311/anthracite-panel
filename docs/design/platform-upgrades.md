# Platform upgrades

Status: in progress. This document is the plan of record and the progress log
for the multi-step upgrade of the panel. It is also the context handoff: a later
conversation should be able to continue from here without re-reading the whole
repository.

## Agreed decisions

| # | Decision |
| --- | --- |
| 1 | Order: foundation → utilities → auth → windows → processes → files → network → partitions → plugins |
| 2 | Auth bootstraps an `admin` account with a random password printed once; enabled in release too; tests build `Config` with `auth.enabled = false` |
| 3 | JWT covers the HTTP and WebSocket interface only; the Unix socket relies on file permissions |
| 4 | JWT is implemented in-tree (HS256 over `hmac` + `sha2` + `base64`) to keep the musl build free of C dependencies |
| 5 | The file editor uses CodeMirror 6, loaded with a dynamic `import()` |
| 6 | Archives: `zip`, `tar`, `tar.gz`; `.xz` and `.zstd` are out of scope for this version |
| 7 | Partitions use `sfdisk`; `mkfs` is supported behind its own switch |
| 8 | Recycle bin is per directory (`.anth-trash/` beside the entries) |
| 9 | `bun` only; `npm` and `npx` are uninstalled |
| 10 | Renames drop the old custom element names, with no compatibility alias |
| 11 | UI source language is English; Spanish ships through `lit-localize`; all code, comments and docs stay in English |
| 12 | Passwords use PBKDF2-HMAC-SHA256 with a random salt, in-tree over `hmac` + `sha2` |
| 13 | `cookie_secure` defaults to `false`, which is what a loopback bind needs; a deployment behind TLS sets it to `true` |
| 14 | Process spawn is gated by a binary allowlist; an empty list denies every spawn |
| 15 | Network interfaces support create, edit, rename and delete, rename only while the link is down |
| 16 | Partitions support create, edit, resize and delete, plus `mkfs` behind its switch |
| 17 | A plugin manifest requires a binary `sha256`; a signature is optional and its absence is warned about, not rejected |

Additional decisions taken while planning:

- Reviews: Copilot review plus a self review with the `code-review` skill. After
  two rounds that only raise minor findings, the remaining items are documented
  and the pull request is merged to keep moving.
- Review waits: after opening a pull request the agent waits up to two hours in
  fifteen-minute intervals. If no review arrives in that window, the next phase
  starts on a branch stacked on the unreviewed one instead of waiting further.
- Branches: `feat/<n>-<slug>` from `main`, one pull request per point, merged
  into `main` by the agent.
- `terminal.token` is removed once the global JWT middleware lands; it stays as
  an accepted-but-ignored key until then.
- Routing is hash based (`#/panel/<id>`).
- `mkfs` types: `ext4`, `xfs`, `vfat`, `exfat`, `btrfs`, `swap`, each behind
  `[volumes] format = false`.
- Plugins: `plugin.toml` manifest, one long-lived JSON-lines process per plugin
  binary, one plugin per zip, binary must match the host architecture. Plugins
  never extend the `anth-proto` enums; they receive `Request::Plugin`.
- Trash layout: `<directory>/.anth-trash/<id>/` with `meta.json`, hidden from
  normal listings, with a configurable retention size.

## Phases

| Phase | Scope | State |
| --- | --- | --- |
| 0 | Handler-forward dispatch, `/api` 404, app shell, i18n rig, bun toolchain | done |
| 1 (point 5) | `anth-modal`, `anth-dialog`, `anth-tab-panel`, harness migration | done, in review |
| 2 (point 7) | JWT auth, refresh rotation, login screen, config commands and settings harness | not started |
| 3 (point 6) | Floating windows, minimize, standalone window | not started |
| 4 (point 1) | `anth-process-manager` and process spawning | not started |
| 5 (point 2) | `anth-file-manager`: CodeMirror editor, archives, recycle bin | not started |
| 6 (point 3) | `anth-network-manager`: interface create/edit/delete | not started |
| 7 (point 4) | Partition create/edit/resize/delete and `mkfs` | not started |
| 8 (point 8) | Plugin system and the WireGuard plugin | not started |

## Phase 0 — foundation (done)

What changed:

- `backend/anth-panel/src/handler.rs` is new. `Handler` has `handle` (returns
  `None` when it does not recognise the request) and `blocks` (whether the
  request may block). `dispatch` walks the chain and answers a request no
  handler accepted with a readable error. `CoreHandler` holds every command the
  panel implements itself.
- `daemon.rs` no longer keeps a second `match` over the request set. The
  connection loop owns the stateful requests (uploads and terminal sessions) and
  asks the chain for everything else, including whether it needs
  `spawn_blocking`. Adding a command now touches the protocol, the client, the
  CLI and one handler arm.
- Unknown paths under `/api` answer `404 {"error": ...}` instead of the SPA's
  `index.html`, so a route typo fails at the status code instead of at the JSON
  parse.
- The frontend has an app shell: `src/app/anth-app.ts` (header, home page of
  cards, panel host, language selector), `src/app/panels.ts` (the registry the
  shell renders), `src/app/router.ts` (pure hash routing) and `src/app/i18n.ts`
  (idempotent `lit-localize` runtime configuration). `index.html` now mounts the
  shell; the Vite scaffold component is gone.
- The shell injects clients through a `clientFactory` property, so tests mount
  every panel against the existing fake clients and no test opens a socket.
- `lit-localize` is wired: `bun run i18n:extract` writes `frontend/xliff/es.xlf`
  and `bun run build` regenerates `src/generated/locales/es.ts` before type
  checking and bundling.
- Toolchain: `bun` 1.4.3 replaces `npm`, `npx` and `corepack`. `bun install`,
  `bun run test` and `bun run build` are the supported commands.

Verification: 448 backend tests, 289 frontend tests, `cargo fmt --check`,
`cargo clippy --workspace --all-targets`, `bun run build`, and a real
`anth-panel run` serving the embedded UI, a live process table and the Spanish
locale.

Notes for the next phase:

- The panel registry is the place to declare a new panel; the shell needs no
  change. The window manager in phase 3 will render the same entries.
- `AppState` still copies the configuration. Phase 2 turns it into
  `Arc<RwLock<Config>>` when the config endpoints land.
- The handler chain has room for a plugin link; phase 8 appends it after
  `CoreHandler`.
