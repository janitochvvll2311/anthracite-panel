# 30. Conventions of the official apps

> Part of the [design index](../../DESIGN.md). §7 defines the contract every app
> satisfies; this document describes how the official catalogue implements it.
> A third party may differ on the choices here — the conformance kit only
> enforces the contract — but a new official app should look and behave like the
> ones already shipped.

The reference implementations: `apps/certs` (an `admin` service with custody),
`apps/proxy` (a `socket` data plane with a document), `apps/net` and `apps/home`
(streams), `apps/fs` and `apps/terminal` (filled, mobile-first views),
`apps/accounts` and `apps/firewall` (forms and modals).

## 30.1 The shape of an app

```
apps/<app>/
  backend/          native service: its own Cargo workspace, bare [workspace]
  frontend/         Bun workspace; dist/<app>.js and <app>-modal.js committed
  wit/<app>.wit     the app's package; no wit/deps in the tree
  manifest.toml.in  placeholders rendered by build.sh
  build.sh          frontend + service + bundle, executable
  README.md         authority, surface, security, tests, decisions
```

- A native backend is a **standalone** Cargo workspace. The only path
  dependencies are `engine/contract` and `sdk/rust/service`; never the panel SDK
  or engine internals. The workspace boundary is the enforcement, not a lint.
- `frontend/dist/*.js` is committed so the bundle builder does not need Bun.
  `dist/<app>.anth` is an artifact and stays in `.gitignore`.
- The eight demo apps share `demo/emulator` as their backend; `certs` and
  `proxy` are native-only. Their `manifest.toml.in` and frontends are shared.
- A `build.sh` mirrors `apps/certs/build.sh`: it installs and builds the
  frontend, builds the release service, hashes the final bytes, renders the
  manifest and zips a closed layout under `/tmp/anthracite-panel`.

## 30.2 The manifest is the only index

- Use `manifest_version = 4` for a service that declares `roots`, `listen`,
  `egress` or `{broker}`; otherwise a v3 manifest keeps its exact meaning.
- `argv` contains exactly one `{socket}`, at most one `{data}`, `{shared}` and
  `{broker}`. The validator allows one `{shared}`, so derive subdirectories in
  the process (`{shared}/tls`, `{shared}/acme/http-01`) instead of repeating it.
- The service class is the authority the operator approves: `socket` for an
  HTTP data plane, `admin` for privileged operations, `host` for host facts,
  `exec` for process execution. The approval flags follow from the class.
- One `[[route]]` per WIT function, with the same interface and function name.
  `GET`/`HEAD`/`DELETE` take query parameters; other methods take a JSON object
  keyed by parameter name. A record stream is `stream = true` + `events = true`;
  a byte stream is `stream = true`; a `future` is a plain route.
- Add a `[[cli]]` verb only for a read or reload action with a matching route.
  A streaming route gets no verb: `anth` would block on it.
- A long-lived stream declares `[limits] bytes-total` / `bytes-per-sec`; the
  access log (proxy) is the only lossy stream and reports `dropped`.
- Slots are declarative: one `panel.view` first, then `panel.fab` with 1–3
  actions and one `panel.modal` per form. A FAB `target` must name a modal of
  the same app; a local `action` reaches the view. Icons are names from the
  panel set, or `web/*.svg` with a pinned hash.

## 30.3 WIT

- Package `<namespace>@1.0.0`, exactly one world `app`, one interface per app.
  `[app] namespace` and the package name are the same.
- `use anth:error/kinds@1.0.0.{error}` for every fallible function. Records use
  kebab-case fields; the JSON on the wire uses the same keys.
- `result<T, error>` for a mutation, `option<T>` for a lookup, `list<T>` for a
  snapshot, `stream<T>` for live data. No new type system on the side.
- Do not add `wit/deps` to the tree: the engine injects its catalogue when it
  parses the bundle.

## 30.4 The service half

- `anth_service::serve(&socket, handler)` owns the socket, `/health` and
  `/__anth/call`. Implement `Handler::call` (synchronous; the SDK runs it on
  the blocking pool) and `Handler::stream` (must not block in `poll_next`).
- Parse `--socket`, `--data`, `--shared`, `--broker` and the class-specific
  arguments from `argv`; there is no environment or stdio. A missing required
  argument exits 2. Unknown extras are currently ignored, like the shipped
  services do: a strict parser in `anth-service` is still pending, and until it
  lands a service must not depend on unknown arguments being rejected.
- Return `serde_json::Value` with kebab-case keys matching the WIT records; use
  the contract error constructors (`invalid`, `not_found`, `unavailable`,
  `conflict`, …). Validation errors name the field path.
- Own state under `{data}` (0700, files 0600), published material under
  `{shared}`, and atomic writes as a temp in the same directory + `sync_all` +
  `rename`. Never hand a filesystem path back through WIT.
- Audit mutations through `{broker}` with `anth:audit/record@1.0.0`; the audit
  is best-effort and never changes the operation's outcome. Secrets, keys and
  headers do not go in the detail.
- Network services bind the argv listeners only, cap live connections, time out
  handshakes and upstream reads, and bound every queue or stream. Long work runs
  in a spawned task; the request path stays responsive.

## 30.5 The web half: one family

An app module runs in a sandboxed frame, in its own document. It does not share
the panel's DOM or stylesheet, so the frame mirrors the palette as CSS custom
properties. **Use the tokens, not new hex values**, and the module will look
like the rest of the product:

```
--bg #0b0d12   --bg-soft #0f1219   --panel #141821   --panel-2 #1a1f2b
--panel-3 #232938   --line #2a3142   --line-soft #202634
--text #e8ebf2   --muted #8f98ab   --muted-2 #667087
--accent #7aa2ff   --accent-2 #9d7bff
--good #52d1a4   --warn #f3c969   --danger #ff7d8a
--shadow 0 24px 70px #00000073   --radius 10px
```

The visual grammar every official view follows:

- **Layout.** A root flex column with `gap: 14px`; a top bar (tabs or title +
  actions) and a `role="status"` line. Content is cards or rows, never a bare
  table. The root prefix is short and app-specific (`.ct`, `.px`, `.fw`).
- **Type.** The frame's 14 px system font; labels 11–12 px uppercase muted;
  ids, paths and fingerprints in `ui-monospace` at 12 px. No external fonts.
- **Colour.** The tokens above; the accent marks focus, selection and links,
  never decoration. Status is `good` / `warn` / `danger`; a neutral badge uses
  `--muted`. `apps/proxy` is fully tokenised and is the example to copy. The
  older modules (`certs`, `firewall`, `accounts`) still carry literal hex values
  from before the frame mirrored the palette; new work uses the tokens, and
  migrating those modules is pending.
- **Spacing and shape.** 4/8/12/14 px gaps, 1 px borders in `--line`, radii 7 px
  for controls and 9–12 px for cards. Shadows are the panel's, not a new one.
- **Buttons.** 12 px, 5–6 px vertical padding, 10–12 px horizontal, radius 7,
  `--line` border on `--panel-2`; `primary` uses the accent-tinted background;
  `danger` uses the danger border/text. Disabled buttons stay visible and say
  why in a tooltip.
- **Badges.** Pills with 11 px text: `ok` (good), `warn`, `bad`, and a muted
  `off`; the same words as the rest of the panel.
- **Forms.** Label above the control; 12–13 px inputs with the same border and
  radius; inline validation in the status line; Enter submits; destructive
  confirmations use a real `<dialog>` inside the app, not `confirm()`.
- **Rows.** A grid with a muted head row, 8–10 px padding, hover border on
  `--line`, selection on the accent. Primary identity in monospace, secondary
  fields muted.
- **Empty, loading, error.** Say it explicitly: an empty view names what is
  missing, a loading view says what it is reading, and an error keeps the code
  and message. Never leave a blank pane or a silent drop.
- **Modals.** Add and Edit use the panel's `panel.modal` slots; the form and the
  panel's other dialogs share the same field and button grammar. On mobile the
  actions stack full width. A view that has no modal slot is read-only and says
  so; it does not fall back to an inline editor with a different look.
- **FAB.** Declarative, icon + label, one to three actions. Add actions target a
  modal; the view hides its own Add button when the FAB exists. Edit stays on
  the row and opens the same modal with the record in `params`.
- **Responsive.** Adapt to the frame width *and* the panel's explicit
  `html[data-viewport="desktop|tablet|mobile"]`: two columns in tablet, one in
  mobile, secondary log columns hidden, status and Apply still reachable. The
  frame is a phone when the panel is in mobile mode even on a wide browser.
- **Accessibility.** `role="tablist"`/`aria-selected` for tabs, `aria-live` for
  status, `aria-label` when a button is icon-only, and Escape closes the
  innermost dialog.
- **Test hooks.** Stable `data-testid` values for the controls an app e2e drives
  (`proxy-view`, `proxy-host`, `proxy-apply`, …); the spec maps them in a page
  object and never selects a class. Panel-wide ids live in §12's testing map
  (`sdk/ts/panel/src/testids.ts`).

The module contract itself: `mount(root, ctx)` synchronously returns a dispose
function, does not await the network before returning, and uses `ctx.fetch` only
under `/api/apps/<its-own-id>/`. Exports `onParams`, `onVisibility`, `onAction`,
`beforeClose` or `snapshot` only when the view needs them. Streams are consumed
with the SSE parser and cancelled on hide/dispose.

## 30.6 Data flow and drafts

- The document or snapshot is read with `ctx.fetch`, rendered once, and refreshed
  on `refresh`/visibility. A mutation is applied explicitly (Apply) and the
  answer replaces the baseline; the draft is a clone until then.
- A modal returns a piece of the draft with `ctx.openView({ params })`; the view
  merges it, and the panel's automatic `refresh` after a modal must not reload
  the server document over that draft.
- Adding is not upserting: an add with an existing id is refused and leaves the
  record (and its children) untouched. Editing replaces in place.
- A failing apply keeps the previous document serving; the view shows the field
  path from the error and the panel's `last-error` banner.

## 30.7 Tests

- **Backend unit tests** use a scratch directory under
  `/tmp/anthracite-panel/<app>-<name>-<pid>`, clean up, and avoid the host. Host
  reads are guarded or routed through a fixture (see `apps/net`, `apps/proc`).
- **Loopback integration** for a network service starts the real listeners on
  `127.0.0.1:0`, reads the assigned port from the service, and drives HTTP,
  TLS/SNI, WebSocket and the stream where applicable (`apps/proxy`).
- **Frontend unit tests** cover pure helpers (SSE parsing, state merges,
  precedence) with `bun test`; no DOM.
- **Bundle conformance** runs `anth-conformance` on the built bundle; the CLI
  parity golden test in `engine/api` checks every `[[cli]]` has a route.
- **Browser e2e** is for the shell and a contributed slot, not every view. An app
  with a real panel flow adds a spec that skips when the app is not composed, and
  drives it through the POM/test-id pattern (§12).

## 30.8 Build, release and docs

- `build.sh` is the only build entry; CI never needs Bun for a committed bundle.
  Sign after rendering the manifest (the signature covers `manifest.toml` and
  `wit/**`), then run conformance.
- Shipping an app in the release/demo is phase 6 work: add it to the release
  scripts and `demo/emulator` with a fixture, and update the golden system
  classes. Do not add `certs` or `proxy` to `demo/composition.toml` without that
  fixture.
- `apps/<app>/README.md` records: authority (argv, roots, egress), the document
  or surface, security rules, tests and the decisions that differ from §7/§25.
  Global lists and build commands live in `apps/README.md`.
- A deviation that changes a contract, the document or the UI family gets an ADR
  in `docs/decisions/` (see 0001–0003 for the proxy).

## 30.9 Checklist for a new official app

- [ ] `backend/` standalone workspace; only `anth-contract` + `anth-service`.
- [ ] WIT package = namespace, one world, error types, kebab-case.
- [ ] v4 manifest with argv placeholders, routes, events/streams, slots, CLI and
      limits; conformance is green.
- [ ] `{data}` 0600/0700 atomic writes; published material under `{shared}`;
      mutations audited.
- [ ] Panel module: tokens, 14 px layout, status line, cards/rows, modals for
      add/edit, FAB actions, mobile/tablet, empty/loading/error states.
- [ ] `data-testid` hooks for the flow an e2e drives; spec skips when uncomposed.
- [ ] `README.md` with authority, surface, security and tests; app listed in
      `apps/README.md`.
- [ ] `build.sh` + conformance; release wiring left to phase 6.
