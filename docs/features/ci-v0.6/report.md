# Report — CI on v0.6.0

## Summary

The workflow now runs for `v0.6.0` pushes and pull requests instead of only
`v0.5.0`. It also covers the demo emulator and the two new build-time tools:
the conformance kit and `anth app fetch-wit`.

## Delivered

- `push`/`pull_request` branches include `v0.6.0`.
- A `demo` job runs `fmt`, `clippy -D warnings` and `cargo test` for
  `demo/emulator`.
- The panel job builds `anth-conformance` and validates the packaged `hello`
  bundle before the remote-client acid test.
- The remote-client test installs `hello`, fetches its WIT with
  `anth app fetch-wit`, checks `wit/hello.wit`, then approves and composes it.

## Decisions

- Keep the existing `v0.5.0` trigger so the old release branch still gets the
  same checks.
- The conformance smoke uses the bundle the panel job already packages; no
  second build path.
- The fetch-wit smoke runs against the real engine in the remote-client flow, not
  a mock.

## Out of scope

- Browser e2e in CI; the panel QA remains manual.
- Real host services and privileged operations.
- Release signing and the distribution key.

## Pending and risks

- The workflow only proves itself when this PR runs on GitHub; there is no
  local YAML validator in the container.
- The panel job is longer now: conformance + fetch-wit add a compile and one
  command, not a service boot.
