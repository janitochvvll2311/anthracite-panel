# Report — Distribution key and the system tier

## Summary

The engine can now derive the `system` tier from a **compiled-in** Ed25519
public key instead of assuming every signed bundle is third-party. The tier is a
property of the signature and of the binary, never of a file an operator or a
bundle can write. A build that has no key has no `system` tier and says so.

This delivers the mechanism, not the release ceremony: the key pair, the signing
step and the rotation runbook still belong to the release process and are the
next PR (see *Pending*).

## Delivered

- `anth_compose::keys::Distribution`: the public key, its fingerprint and
  `tier(signer)`. `Distribution::embedded()` reads
  `ANTHRACITE_DISTRIBUTION_KEY` at compile time; `from_public_key` exists for
  tests and tooling.
- Install order is **distribution first, keyring second**:
  - the distribution fingerprint earns `system`;
  - any other trusted key still earns `third-party`;
  - the keyring can no longer be the only path to verify a release bundle, which
    is what makes the tier reachable without handing the operator privilege.
- `bundle::StagedBundle` carries the tier it earned, so the registry writes
  `descriptor.tier` from the signature instead of hardcoding `third-party`.
  `install` and `replace` now share `descriptor_for`, which also applies the
  service switch to the derived tier.
- A system bundle with `[service]` installs without
  `[services] allow-third-party`: the switch exists for third parties, not for
  the release.
- `read_descriptor` validates a stored `system` descriptor against the compiled
  key. A hand-edited `tier = "system"` with another signer is refused, and a
  build without the key reports that it has no `system` tier instead of guessing.
- A malformed compiled key refuses **every** install with an actionable message;
  a broken build must not silently demote release bundles.
- `GET /api/engine/keys/distribution` → `{fingerprint, public_key, problem}` and
  `anth key distribution` make the trusted key visible to the operator.
- `anth-conformance` reports the `tier` it would earn on that build, in text and
  JSON, so a third-party author sees the verdict without a daemon.

## Tests

- `engine/compose`: **162 passed**, including
  - `the_distribution_key_mints_the_system_tier` (system service with no
    keyring and no `allow-third-party`; a keyless build refuses the descriptor),
  - `a_keyring_key_cannot_mint_the_system_tier` (trusted signer, still
    `third-party`, still gated),
  - `a_descriptor_cannot_claim_the_system_tier` (hand-edited tier is refused),
  - `an_unusable_distribution_key_refuses_installs`.
- `engine/api`: **96 passed**, including the distribution endpoint.
- `sdk/rust/cli`: **59 passed**, including the new verb and its route.
- `sdk/rust/conformance`: **7 passed** with the `tier` field.
- `cargo fmt --check` and `clippy -D warnings` on the touched workspaces.

## Decisions

- **The tier is derived from the signer, never declared.** The manifest is frozen
  at v2/v3 and must not gain a privileged field; a bundle cannot ask for a tier.
  The descriptor records the outcome, and reading it re-checks the signature
  fingerprint.
- **The distribution key is a build input, not configuration.** A config key
  would let an operator mint a `system` app, which contradicts
  [§9.1](../../design/09-authority.md#91-tiers-and-signing).
- **A broken build fails closed on install.** Refusing the install is louder and
  safer than treating a release bundle as third-party.
- **A keyless build is a legitimate debug build.** The demo and the panel keep
  installing repo bundles as `third-party`; nothing about v0.6.0 development
  needs a private key.

## Out of scope

- The release tool that produces `manifest.sig` (next PR).
- Signing the shipped `apps/*` bundles and making them `system` in a release.
- Key rotation mechanics beyond "it is a release": there is still no CRL.
- Enforcing namespace ownership (`anth`, `anthracite`) by tier. The rule is
  documented in [§7](../../design/07-app.md) and the repo apps still install as
  third-party in debug, so enforcement waits for release signing.

## Pending and risks

- **No signer in the repository.** A third party and the release process both
  need to produce `manifest.sig`; the canonical message is specified and
  implemented once (`anth_contract::signature::message`), but nothing writes the
  file. This is the next PR and a hard prerequisite for the external acid test.
- **Namespace ownership is still unenforced**, so an unsigned third-party bundle
  may declare `anthracite:hello`. The tier does not gate it yet; it is a
  documented gap, not a regression.
- **Rotation is a release** and a fingerprint change invalidates every installed
  system bundle at once: on the next boot the catalog reports each of them as a
  problem. A rotation runbook has to re-sign and reinstall, and it should be
  rehearsed before a deployment relies on the tier.
- **The compiled key is public but the build is not reproducible from the repo**:
  two builds with different keys produce binaries that trust different signers
  and nothing in the artifact records which key it carries. The
  `/keys/distribution` fingerprint is the current answer; a release manifest
  should record it too.
