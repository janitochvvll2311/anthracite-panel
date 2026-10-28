# AGENTS.md

## Current state

- `v0.3.0` is the rewrite branch. Phase 0, the Phase 1 host-info slice, the Phase 2 auth/lifecycle slice, the Phase 3 Files slice and the Phase 4 terminal and process slices are in place: daemon socket with `Hello`, CLI `host-info`, HTTP + SSE, minimal auth, settings API/page, session revocation, restart/reboot and a Lit shell with the v0.2 layout (sidebar, drawer, tab strip, FAB) plus host bars and refresh/pause controls. Files supports listing, text and image previews, a tabbed editor with create/edit/save, a transfer queue for uploads and downloads, deletion behind `files.write`, the freedesktop.org trash (move, list, restore, empty), and in-process archives (zip, tar, tar.gz, tar.xz, read-only 7z) over socket, HTTP, CLI and the web UI. Terminal supports PTY sessions with owner/seq/idle limits, a socket read contract, a browser WebSocket bridge, a global tabbed xterm window and a mobile key bar/gestures. Processes supports paginated `/proc` snapshots, an SSE watch feed, allowlisted signals guarded by `pid + start_ticks`, allowlisted detached spawns, and a flat/tree manager in the web UI. Phase 5 is code-complete (real-command validation pending): the network inventory (sysfs + `getifaddrs`, cumulative counters, default routes and client-derived rates), its guarded up/down, create, edit, rename and delete through direct `ip` argv, and the managed `inet anthracite` firewall with `input` and `output` chains, allow/deny/edit/delete/flush rules and per-chain default policies through direct `nft` argv are in place over socket, HTTP, CLI and the Network section's Interfaces and Firewall tabs. Phase 6 is code-complete: the Storage inventory (block devices, mounts, usage and swap), guarded mount/unmount, partition create/resize/delete and format through `sfdisk`/`mkfs`, and the Accounts inventory plus guarded user/group create/delete and password changes through shadow-utils (`chpasswd` on stdin) over socket, HTTP, CLI and the Storage/Accounts tabs. Accounts success paths were validated against the real host; storage mutations still need `CAP_SYS_ADMIN`. Phase 7 has started with the signed bundle format, per-file hashes, frozen grants, install/list/enable/disable/remove/approve over local CLI and HTTP, the Settings → Plugins panel, the `wasmi` ABI 3 runtime, guarded HTTP anchors, declared UI asset delivery and the sandboxed iframe bridge; the `hello`/WireGuard reference bundles are next.
- The plan of record is `docs/design/v0.3.0.md`. The accepted Phase 0 contracts are in `docs/design/phase-0-contracts.md`. The architecture reference for the two previous trees is `docs/design/reference-v0.1-v0.2.md`.
- Prior code lives on other refs:
  - `origin/v0.2.0` — newest previous implementation: Rust workspace `backend/` (`anth-wire`, `anth-host`, `anth-plugin`, `anth-panel`), Bun workspace `frontend/`, `plugins/hello/`, frozen `v1/`, and CI in `.github/workflows/v2.yml`.
  - `origin/main` — shipped v0.1 tree: `backend/` (`anth-proto`, `anth-panel`), Vite/Lit `frontend/`, `plugins/wireguard/`, and `docs/`.
- Read prior context without changing branches:

  ```bash
  git show origin/v0.2.0:AGENTS.md
  git show origin/v0.2.0:README.md
  git show origin/v0.2.0:docs/design/v0.2.0.md
  git show origin/main:AGENTS.md
  ```

  Those files describe those trees, not this one.

## Toolchains

- Rust, rustup and Cargo live under `/tools` (`CARGO_HOME=/tools/cargo`, `RUSTUP_HOME=/tools/rustup`), pinned by `rust-toolchain.toml` to `1.99.1` with the `x86_64-unknown-linux-musl` target, `rustfmt` and `clippy`.
- Bun lives under `/tools/bun` (`BUN_INSTALL=/tools/bun`), version 1.4.4.
- `/tools/env.sh` exports the paths. `/usr/local/bin` wrappers source it, so `cargo`, `rustc`, `rustup`, `rustfmt`, `cargo-fmt`, `cargo-clippy` and `bun` work from a non-login shell. If a wrapper is missing after an environment rebuild, run `. /tools/env.sh` first.
- The system C toolchain (`gcc`, `musl-dev`, `make`, `pkgconf`) is installed with `apk`; it is not under `/tools` and may need reinstalling after a full image rebuild.

## Build and test

Run backend commands from `backend/`. The workspace pins `x86_64-unknown-linux-musl`; a bare `cargo build` is correct.

```bash
cd backend
cargo build --workspace
cargo test --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

A release build embeds `backend/anth-panel/www/`. Build the shell first, or `build.rs` refuses the release build:

```bash
cd frontend
bun install
bun run test
bun run typecheck
bun run build
./build-shell.sh

cd ../backend
cargo test --workspace --release
```

Focused checks:

```bash
cargo test -p anth-wire
cargo test -p anth-host
cargo test -p anth-panel
cargo test -p anth-panel --test host_info
cargo run -q -p anth-panel -- run --addr 127.0.0.1:8080 --config /tmp/anthracite-panel/run/config.toml
cargo run -q -p anth-panel -- host-info --config /tmp/anthracite-panel/run/config.toml
```

Debug builds default to relative paths (`./config.toml`, `./anth.sock`, `./state`, `./plugins`, `./plugins.d`, `./secrets.toml`). Release defaults use `/etc/anth/*` and `/var/lib/anth/*`. `Config::load_or_create` writes a missing config; `Config::set_key` edits one dotted key through `toml_edit` and preserves comments. `www/` is gitignored apart from `.gitkeep`; `frontend/build-shell.sh` stages it.

## Working here

- The Phase 0 contracts are accepted; implement against them and update the file if a decision changes.
- Extra tasks that are not phase exit criteria live in `docs/design/v0.3.0.md` §19. Files drag and drop is tracked there, including its `files.move` prerequisite. The same section records that real `ip`/`nft` success paths still need a container with `CAP_NET_ADMIN`/`/dev/net/tun`, and real storage mutations need `CAP_SYS_ADMIN`.
- Use `/tmp/anthracite-panel` for test scratch and manual run state, never the shared `/tmp` root and never `.tmp`.
- Debug builds use `Config::dev_default()`: relative paths and every policy switch on, including the sensitive ones (`terminal`, `process.spawn`, `network.manage`, `firewall.control`, `volumes.partition/format`, `accounts.manage_passwords/allow_system/allow_privileged_groups`, unsigned plugin installs, file writes and trash). Release defaults stay closed. `volumes.disk_allow` remains empty because it is an explicit device choice; add a throwaway disk before testing partition edits.
- `./scripts/dev.sh start|stop|status` runs the daemon and Vite together; its config, logs and PIDs live under `/tmp/anthracite-panel/dev`. `ANTH_ADDR`, `ANTH_VITE_HOST` and `ANTH_VITE_PORT` override the defaults. HMR is off by default so phone testing survives the native file picker (Android drops the HMR socket and Vite reloads on return); set `ANTH_HMR=on` for desktop fast refresh. The helper builds with `cargo build` as your user and then runs the compiled binary through `sudo` from the directory you invoked it in, because network/firewall control needs `CAP_NET_ADMIN`; `stop` and `restart` signal the root daemon through `sudo` too. Sudo cannot add a capability the container's bounding set excludes. If sudo would prompt without a terminal, the helper exits with an error instead of waiting.
- `docs/design/ui-reference.md` records the approved Network section as the template for new feature sections: one feed, toolbar state/count/interval/pause, a context strip, a local filter, responsive entity cards and client-derived values. Firewall, Volumes and Accounts follow it.
- `docs/design/cli-http-parity.md` is the socket/HTTP/CLI coverage matrix. Add its row when a wire operation lands; transport-only routes go in its exception table with a reason.
- Docs are written in English, matching the previous trees. Keep them in sync with code as it lands.
- The root `.gitignore` covers local debug config, secrets, state, plugin directories, `frontend/node_modules`, `frontend/dist` and the staged `www/`.
- License is Apache-2.0 (`LICENSE`).
- Do not switch, merge or rebase branches unless asked. Feature branches start from and target `v0.3.0`; `main` still holds v0.1.
