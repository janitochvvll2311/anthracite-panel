# Browser e2e

Playwright against a **running panel**. The suite is deliberately small: design
§12 asks for the official panel and one contributed slot, not a second copy of
the unit tests. (`remote-client.sh` in this directory is the transport-level
check for the generated Rust and TS clients; it is not browser e2e.)

## The pattern

- **Page objects.** `src/pages/panel.page.ts` and `src/pages/login.page.ts` map
  the two screens. Each key component has a wrapper in `src/components/`
  (`sidebar`, `panes`, `windows`, `stack`, `settings`, `manager`, `slot-frame`,
  …). A spec calls business-level methods (`panel.openFirstView()`,
  `panel.windows.maximize(window)`) and never writes a selector.
- **Test ids.** Selectors go through `TESTID`, exported by
  `sdk/ts/panel/src/testids.ts` and re-exported by `src/testids.ts`. The panel
  shell applies them with `testId()`, the frame sets them on its body, and the
  login/setup pages in `panel/backend` carry the same literals. A test id says
  *what* a node is; the node's `data-app-id` / `data-tag` / `data-window-tool`
  says *which* contribution it belongs to. **No CSS class is ever a selector.**
- **State, not labels.** Assertions read `data-state`, `data-mode`,
  `data-active`, `aria-selected` and identity attributes. Labels, versions and
  health text come from the composition and are never asserted.
- **No sleeps.** The shell marks `body[data-shell-state="ready"]` after the
  first reconciled surface; a frame marks `data-state="mounted"` after the
  module resolved, and its body names the contribution. Wait on those.

`src/fixtures.ts` logs the operator in once (global setup), saves the session
to `.auth/operator.json` and exposes `panel` (already reconciled) and `login`
(the public gate). `auth.spec.ts` opts out of the stored session with
`test.use({ storageState: { cookies: [], origins: [] } })`.

`containers.spec.ts` and `store.spec.ts` follow the same pattern and are gated by
the fixture variables above; the store manager test deletes the signer key first
so the handoff prompt is exercised on every run.

`proxy.spec.ts` runs only when the composition carries `org.anthracite.proxy`
(the standard demo does not until the v0.7 release wiring) and is skipped with
a message otherwise. To exercise it, build and install the bundle, approve it
and recompose, then restart the panel:

```bash
apps/proxy/build.sh
anth --socket <state>/engine.sock app install apps/proxy/dist/proxy.anth
anth --socket <state>/engine.sock app approve org.anthracite.proxy \
    --accept-write --accept-admin
anth --socket <state>/engine.sock compose --add org.anthracite.proxy --now
```

## Run it

The panel must be running and **past first boot**: the first boot `/setup` page
rotates the generated password, and the suite does not mutate credentials.

```bash
# build the shell once: the debug panel reads panel/frontend/dist from disk
(cd panel/frontend && bun install && bun run build)

# somewhere: start a panel. The developer demo is the usual fixture:
cargo run --manifest-path panel/backend/Cargo.toml -- \
    --config demo/config.toml --listen 127.0.0.1:5173

cd tests/e2e
bun install

ANTH_PANEL_URL=http://127.0.0.1:5173 \
ANTH_PANEL_PASSWORD=<the operator password the panel accepts> \
    bun run test
```

A fresh panel writes a generated password to `<state>/credentials.txt` and
requires `/setup` before the shell opens. Open `/setup` once, choose a password,
and pass that one: the suite does not rotate credentials.

| Variable | Meaning |
| --- | --- |
| `ANTH_PANEL_URL` | Panel origin. Default `http://127.0.0.1:5173`. |
| `ANTH_PANEL_OPERATOR` | Operator name. Default `operator`. |
| `ANTH_PANEL_PASSWORD` | Required. The operator password the panel accepts. |
| `ANTH_CHROMIUM` | Chromium path. Default `/usr/bin/chromium` when present. |
| `ANTH_CONTAINERS_FIXTURE` | Set to `1` to run the Containers spec against `scripts/containers-fixture.ts`. |
| `ANTH_STORE_FIXTURE` | Set to `1` to run the Store specs against `scripts/store-fixture.sh`. |
| `ANTH_STORE_PUBLIC_KEY` | The fixture catalogue signer public key (64 hex); the Manager handoff pastes it. |
| `ANTH_STORE_SIGNER` | The signer fingerprint; the spec deletes it from the keyring first so the prompt is deterministic. |

`bun install` does not download a browser. If the host has no system Chromium
and `ANTH_CHROMIUM` is unset, install Playwright's once:

```bash
bunx playwright install chromium
```

```bash
bun run test:headed   # watch it
bun run test:ui       # Playwright UI
bun run typecheck     # tsc --noEmit
bun run report        # last HTML report (CI only)
```

## Adding a test

1. If the test needs a hook that does not exist, add the id to `TESTID` in
   `sdk/ts/panel/src/testids.ts` and apply it with `testId()` where the node is
   created. Never add a test id only to a test.
2. Put the screen operation in the page object or the component wrapper.
3. Write the spec against the page object, asserting state attributes.

The composition poll rebuilds the sidebar, tabs and panes every ten seconds,
so specs must hold locators (retried) and never element handles.
