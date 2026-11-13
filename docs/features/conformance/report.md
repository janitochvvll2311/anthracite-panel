# Report — Conformance kit

## Summary

`anth-conformance` is the offline kit: it validates a bundle with the engine's
own verifier, in a temporary state tree, without a daemon. It checks the zip
layout, manifest, file hashes, WIT package/world, imports and signatures, then
reports what the bundle claims.

## Delivered

- `sdk/rust/conformance` with the `anth-conformance` binary.
- Options:
  - `<bundle.anth>` positional.
  - `--require-signature`.
  - `--allow-third-party-service`.
  - `--known <namespace:package>` for imported provider packages.
  - `--key <hex-ed25519-public-key>` to trust a signer.
  - `--max-bytes <n>` (default 32 MiB).
  - `--json`.
- The kit creates a `0700` scratch state, uses the engine's `bundle::extract`,
  and removes the tree on success or failure.
- The text summary names the app id, version, package, signer, exports, imports
  and service class; `--json` emits the same fields.

## Tests

- `anth-conformance` library: a minimal valid bundle, a tampered component, an
  unknown import refused and a known import accepted, a signed bundle with an
  untrusted signer refused and the same bundle accepted with `--key`, and the
  private/removed scratch directory.
- Binary: option parsing and the required bundle.
- Real smoke: `home-layout-test.anth` validates and reports
  `org.anthracite.home 0.1.0`, package `anthracite:home`, unsigned, service
  `host`.
- The old invalid Home bundle with `web.sed` is refused with the layout error.
- `cargo fmt --check` and `clippy -D warnings` on the SDK workspace.

## Decisions

- Reuse `anth_compose::bundle` instead of reimplementing the checks. The kit and
  the engine must agree; a second implementation would be a second truth.
- The kit is offline by design: no registry, no install, no runtime. It takes a
  local bundle and optional known packages/keys.
- `require_signature` is opt-in, matching a development engine; a release
  deployment can require it explicitly.
- `--key` takes the raw Ed25519 public key in hex and adds it to a temporary
  keyring. A signed bundle still fails if its key is not supplied.
- The scratch state is `0700` and removed after the call; the kit leaves no
  installation behind.

## Out of scope

- Running the component or exercising its routes; that is runtime conformance,
  not bundle conformance.
- A registry or version solver for `--known`.
- Generating keys or signatures; signing is the author's workflow.
- Pulley or host-service validation.

## Pending and risks

- A bundle with a third-party service needs `--allow-third-party-service`; that
  is deliberate and mirrors the engine's default.
- The kit validates what a bundle claims; it does not prove that a provider
  package with that name is the one the operator will compose.
