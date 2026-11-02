# 16. Decision log

> Part of the [design index](../../DESIGN.md).

Decisions taken during the v0.5.0 review. Binding until superseded.

| ID | Decision | Why |
| --- | --- | --- |
| R1.1 | v0.5.0 ends at M3; features return in v0.6.0 | The platform is judged by its contract, not its catalogue |
| R1.2 | WebAssembly Component Model + WIT from day one | One type system, real tooling, multi-language apps |
| R1.3 | *Superseded by R1.3-rev.* Shell as an app, with a bootstrap document served by the engine | First pass; replaced once the panel became an application |
| R1.4 | Iframe sandbox per app | The authority model matters more than theming convenience |
| R1.5 | Install, approve and compose are separate acts | Three different questions, three auditable states |
| D-Runtime | wasmtime + Cranelift, single backend | Reference implementation; lets wasm do heavy work |
| D-Pulley | Pulley documented, not shipped | W^X hosts are a deployment profile, not the default |
| D-Linking | Direct typed links app→app; broker only for host interfaces | Fighting the linker costs weeks and buys little |
| R2.1 | WIT ids for interfaces; reverse-DNS for apps; app WIT namespace declared | WIT cannot express DNS-shaped names |
| R2.2 | Exact interface versions | What the linker requires; evolve by exporting two versions |
| R2.3 | Manifest is an index; the WIT package lives under `wit/` and is signed (refined by D-Wit-Package) | WIT is a language, not a config string |
| R2.4 | Events are typed notifications, not a topic bus | Confirmed; deferred to v0.6.0 because the mechanism needs streams |
| R2.5 | CLI and clients generated from WIT | Hand-written surfaces drift; v0.4.0 proved it |
| R2.6 | State is interfaces only; never paths | A path is ambient authority |
| R2.7 | `anth:error@1` with a fixed HTTP mapping | One error model for HTTP, CLI and clients |
| R2.8 | The component world is validated against the grant before linking | A link is authority; a lying manifest must not run |
| D-Streams | Streams and typed events land in v0.6.0/M5, designed against the terminal; the runtime is pre-shaped now | A stream designed without a real consumer gets redesigned at the PTY anyway |
| R3.1 | Risk is computed over the resolved import graph, never declared by the manifest | A manifest that declares its own risk can lie about it |
| R3.2 | One interface, one risk level; split mixed interfaces | Grants are per interface, so a mixed interface forces the highest level |
| R3.3 | Distribution key compiled in; the operator keyring only extends `third-party` trust | A config file must never be able to mint privilege |
| R3.4 | An upgrade auto-applies only on an empty authority delta | Re-approving every patch trains operators to click through approvals |
| R3.5 | Static misses fail the composition; runtime failures degrade (refined by D-Fatal-Rule) | A silent degraded boot hides operator mistakes; a crash must not take the engine down |
| R3.6 | No revocation in v0.5.0: uninstall or rotate the distribution key | Honest limitation; a CRL only when a fleet needs it |
| R1.3-rev | The engine serves no UI. The panel is an application that hosts apps | A shell bundled as an app still put UI concerns in the platform; a headless engine keeps the thesis literal |
| D-Workspace | Four Rust workspaces and three Bun ones, with a shared target dir and CI-checked forbidden edges | The isolation must be unrepresentable, not merely documented |
| D-CLI | The CLI lives in `sdk/rust/cli` and builds the `anth` binary; the distribution ships it beside the panel (refined by D-Cli-Surface) | The engine's commands are not a panel's private feature |
| D-Remote | The engine must stay usable by a panel in another process; v0.5.0 ships only the linked mode | The API is the product boundary; an in-process shortcut would quietly close that door |
| D-Target | One shared `build.target-dir` for the Rust workspaces | Separate workspaces buy isolation, not a second compiler |
| D-HTTP | The API is resources, not commands: `POST` installs, `PUT` replaces and approves, `PATCH` toggles state, `DELETE` removes and revokes | Actions exposed as invented verbs drift from the resources they mutate, and cannot be retried safely |
| D-Compose-PUT | The composition is replaced as a whole document with `If-Match` | A read-modify-write of a document needs a precondition, not an `add` endpoint |
| D-Install | `POST /api/engine/apps` creates and derives the id; `PUT /apps/{id}` only replaces | The id lives inside the bundle, so a client cannot name it before uploading |
| D-Services | `service` apps arrive in v0.6.0 with the terminal; v0.5.0 ships wasm and ui | A binary that cannot speak WIT is an exception to the type system, and it needs its first real consumer to be designed |
| D-Slots | The SDK owns the slot vocabulary (`panel.view`, `panel.action`, `panel.settings`); a panel declares which it implements; the engine validates only the format | Portable apps need a contract-owned vocabulary, and the engine must not learn UI |
| D-Blobs | Blobs are not in the v0.5.0 native set; they arrive with transfers in v0.6.0 as `stream<u8>` | The resolver treats the native set as always available, and `bytes` would have to change contract when streams land |
| D-Wit-Package | The WIT package lives implicitly under `wit/`; its name and version come from `[app] namespace` and `version`; exactly one world | One package name written once, and a world that has a home |
| D-Cli | `anth app <alias> <verb>`; the CLI surface comes from the composition; three output views with `--table` by default; exit codes follow `ErrorKind` | Only composed apps can be invoked, and the CLI must not become a generic HTTP client |
| D-Format | TOML for manifest, composition, config and grants; JSON for the API; YAML only as a CLI output view | Implicit typing in a signed artifact is a silent change, and YAML editing drops the comments an operator needs |
| D-Credentials | First boot generates operator name and password into `state/credentials.txt` (`0600`) and forces a change on first login | A default account is a hole, and a secret in the log is a leak |
| D-Health | `/health` is unauthenticated liveness; `/api/health` is authenticated readiness and answers `503` until ready | Liveness and readiness answer different questions; one endpoint cannot be both |
| D-Remote-Impl | Only the linked panel is implemented in v0.5.0; HTTP stays as a designed transport | The SDK keeps the door open without paying for it |
| D-Namespace | `anth` for the engine, `anthracite` for first-party apps, everything else for third parties | A reserved namespace only the engine can use prevents an app from impersonating the host |
| D-Mapping | One HTTP ↔ WIT mapping, implemented once in the router and reused by codegen | Three implementations of the same rule is three chances to disagree |
| D-Slot-Contract | A slot contribution is an ES module exporting `mount(root, ctx) -> cleanup` | Without a written contract, every panel invites a different module shape |
| D-Signature-Message | Signature covers a canonical, sorted, length-prefixed manifest set (`manifest.toml` + every file under `wit/`) | The format must be written once and pinned by a test vector, not inferred from code |
| D-Binding | No provider map: the interface id is the binding, and a second implementation is a different interface | A configuration table with nothing to decide is a source of contradictions |
| D-Failure-Phases | Failures are classified by phase: resolve failures are fatal, instantiation and runtime failures degrade | A graph error and a broken app deserve different severity |
| D-Fatal-Rule | A composition error is fatal only when it can be fixed without the engine running | Otherwise the engine locks the operator out of the tool that repairs it |
| D-Error-Envelope | `{"error":{"code","message","app?","detail?"}}`; the app's six codes plus four engine codes; unknown codes read as `internal` | One contract shared by HTTP, CLI and clients, extensible without breaking readers |
| D-Cli-Surface | The CLI has generated app verbs and a fixed engine command set; `anth status` reads health plus composition | The management half was never specified, which is how a command that does not exist gets cited |
| D-Peer-Auth | The Unix socket authenticates by peer credentials; TCP authenticates by session | The kernel-issued identity needs no stored secret, and remote access keeps a real session |
| D-Acknowledge | Breaking actions return `409` with `detail.acknowledge`; `--acknowledge <token>` proceeds and is audited | In a security contingency the operator must be able to act, and the record of acting knowingly is worth more than a prohibition |
| D-Grant-Scope | A grant freezes what an app imports, not who provides it | The provider changes with the composition, and a grant must not lie when it does |
| D-Risk-Recheck | Risk is computed at approval over installed apps and re-checked at compose; a growth blocks the app until re-approved | An app cannot gain authority because its provider was swapped |

---
| D-Build-Target | Apps build for `wasm32-unknown-unknown` and are wrapped with `wasm-tools component embed` + `component new`; `wasm32-wasip2` is not used | The wasip2 target links WASI interfaces nobody granted, and the world check would refuse every app built with it |
| D-Streams-Contract | Streams and futures are values returned by synchronous functions; `async func` is not used in WIT; concurrency lives inside the guest and in `anth-runtime` | An async export makes asynchrony contagious up to the router and gives HTTP two shapes |
| D-Exhausted | A budget trap maps to `exhausted` (503, exit 5) with `detail.budget`, not `internal` | Running out of granted fuel is a policy outcome, not a bug |
| D-Type-Introspection | The router reads parameter and result types from `component_type()` at runtime; unsupported types are refused at install | An installed app is an artifact, so its signature is read rather than assumed |
