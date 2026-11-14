# Report — The first-party namespace by tier

## Summary

§7.3 reserved `anthracite` for first-party apps and said an app cannot claim it
unless its tier matches, but nothing checked the tier. A third-party bundle could
ship `anthracite:hello/greeting@1.0.0`, and because interface ids are the binding
in a composition, it could stand in for a shipped app. The registry now enforces
the rule against the tier derived from the signature.

## Delivered

- `check_namespace_tier` in the registry: `anth` is already refused by manifest
  validation; `anthracite` is refused for a `third-party` bundle when the build
  carries a distribution key.
- The rule is applied both when a bundle is installed (`descriptor_for`) and when
  an installed tree is read (`read_installed`), so an app installed by a keyless
  build does not survive a move to a signed build — the catalogue reports it as a
  problem instead of the composition running it.
- A keyless build has no `system` tier by design and keeps installing the shipped
  apps as `third-party`. Refusing them there would make every debug build unable
  to install the repository's own apps, and the rule only has a tier to compare
  against when a release key exists.
- Exports must live under the app's own package namespace (§7.3), so enforcing
  the package namespace covers the interfaces the app can export.
- Docs: §7.3 states the enforcement and the keyless exception, §9.1 says the
  third-party tier may not claim the namespace, §16 gains `D-Namespace-Tier`.

## Tests

- `engine/compose`: **171 passed** (4 new):
  - `a_release_build_refuses_the_first_party_namespace_from_third_parties`
    (trusted keyring signer, still refused, nothing published),
  - `the_distribution_key_owns_the_first_party_namespace` (same manifest signed
    by the release key installs as `system`),
  - `a_keyless_build_still_installs_the_shipped_apps`,
  - `a_release_build_refuses_a_first_party_app_installed_as_third_party` (the
    read path, not just install).
- The whole engine workspace also passes with `ANTHRACITE_DISTRIBUTION_KEY` set;
  the golden and system-composition tests now pin a keyless build through
  `EngineState::keyless`, so they state which build they model instead of
  inheriting the compiler environment. `keyless` can only drop the compiled key,
  never install another one, so a host cannot mint `system` at runtime.
- `engine/api`: 96; `panel/backend`: 6; `sdk/rust`: unchanged; fmt, clippy and the
  edge gate clean.

## Decisions

- **Enforce on the tier, not on the name alone.** A name is a claim; the
  distribution signature is the fact.
- **Skip the check in a keyless build.** It is the build that has no `system`
  tier; the alternative is a development build that cannot install its own apps.
- **Apply it on read as well as install.** Changing the engine binary must not
  silently bless an app that was installed under looser rules.

## Out of scope

- Signing the shipped `apps/*` bundles in a release, which is what will let a
  release build install them at all.
- Enforcing app **ids** (`org.anthracite.*`); the id is not authority, the WIT
  namespace is.
- Revocation and rotation.

## Pending and risks

- A keyless build trusts the `anthracite` name for anything installed locally. It
  is a development build with no `system` tier and no release key, so the
  namespace buys nothing there; a deployment that would care runs a keyed build.
- The error is a `denied` on the app, so a composed app that disappears after a
  binary swap shows up in the catalogue as a problem, not as a silent stop.
