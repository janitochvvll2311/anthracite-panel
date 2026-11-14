# Report — The external bundle acid test

## Summary

The definition of done for M5 is an app that lives outside this repository and
still installs, composes, renders and answers. `tests/acid/app` is such an app —
outside `apps/` and outside the apps workspace, with its own `[workspace]` — and
`scripts/acid-test.sh` proves the whole path against a real panel, with no engine
change.

## Delivered

- `tests/acid/app`: a standalone crate (one `anth:env/time` import, one exported
  function, one route, one panel module) that depends on this repository only
  through the `anth-guest` SDK, exactly like a third-party author. Its build
  script mirrors the SDK catalog into `wit/deps/`, which is generated and
  ignored; the bundle itself is packaged without it.
- `scripts/acid-test.sh`:
  1. builds the component with plain `cargo` for `wasm32-unknown-unknown` and
     wraps it with `wasm-tools` — the three documented steps, not the repository's
     `apps/` workspace;
  2. renders the manifest with the component and module hashes, packs and signs
     it with `anth-sign` and a key generated for the run;
  3. runs `anth-conformance --require-signature --key` on the result;
  4. starts the panel with `[apps] require-signature = true`, trusts the key
     through `anth key add`, installs, approves and composes the bundle;
  5. restarts, checks that `anth app ping now` reaches the route over the engine
     socket, that the panel serves `web/ping.js` byte-for-byte with
     `Cache-Control: immutable`, and that the composition surface lists the app
     and its `panel.view` slot.
- CI: the panel job runs `scripts/acid-test.sh`, so the path is checked on every
  push instead of being a one-off.
- Docs: §11 records the CI flow and the remaining "someone who is not us"
  release check, §12 adds the script to the test suite, `AGENTS.md` lists the
  command.

## Tests

- `scripts/acid-test.sh` passes locally: built, signed, verified, installed,
  approved, composed, restarted, answered over the CLI, served the module and
  reported the slot.
- The conformance step inside the script is the existing kit, so the bundle is
  verified by the same code the engine uses.
- No unit tests: this is an integration path, and the script asserts outputs on
  every step (the install would fail on a signature or tier mistake).

## Decisions

- **The app is a standalone workspace.** A crate inside `apps/` would prove
  nothing about an outside author; a path dependency on `anth-guest` is exactly
  what a third party writes.
- **The bundle is signed and `require-signature` is on.** An unsigned acid test
  would skip the trust path that a real deployment uses.
- **The run uses the headless API plus the official panel's HTTP surface for
  assets.** A browser render is the Playwright suite's job; the acid test checks
  the mechanism (module served, hash pinned, slot composed) without a browser.

## Out of scope

- A browser-based render assertion for this app; `tests/e2e` covers the hello app
  and the second panel.
- Signing `apps/*` with a real release key and installing them as `system`.
- Publishing the bundle outside the repository or executing the test with a
  person who is not us.

## Pending and risks

- The script builds the panel and the SDK tools before running; in CI those
  builds are already warm from earlier steps, but a cold run is several minutes.
- The panel's first-boot password ceremony is exercised (login plus password
  change) because the HTTP surface requires a session; a change in that flow
  will fail the script loudly.
