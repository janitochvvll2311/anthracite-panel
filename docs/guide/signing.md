# Signing and tiers

Every installed app has a **tier**, derived from the signer, never declared by
the bundle:

| Tier | Source | May request |
| --- | --- | --- |
| `engine` | compiled in | nothing; it *provides* the native interfaces |
| `system` | signed by the distribution key | wasm, ui, services; the `anthracite` namespace |
| `third-party` | any other signer, approved by the operator | wasm, ui; a service only with `[services] allow-third-party = true`; not `anthracite` in a keyed build |

## The distribution key

The public half is a **build input**:

```sh
ANTHRACITE_DISTRIBUTION_KEY=<64 hex chars> cargo build --release …
```

`anth-compose` reads it at compile time. A release binary carries the key; a
debug build carries none and has no `system` tier. Setting the variable at
runtime does nothing: the key is trusted because the binary was built with it,
not because a file or a request says so. A value that is present but unusable
(not 32 bytes of lowercase hex, not an Ed25519 point, weak) refuses **every**
install instead of demoting release bundles.

The private half never enters the repository or the engine build. Keep it with
the release process, at mode `0600`, and back it up offline.

What a build trusts is public and reportable:

```sh
anth key distribution
# fingerprint 300ac4…
```

or `GET /api/engine/keys/distribution` → `{fingerprint, public_key, problem}`.
Compare the fingerprint with the release notes; a `problem` means the build is
broken and installs are refused.

## The keyring

The keyring at `$state/keys` trusts **third-party** signers:

```sh
anth key add <32-byte-hex-public-key> [--label "…"]
anth key list
anth key remove <fingerprint>
```

The engine derives and prints the fingerprint (`sha256(public key)`); the keyring
extends trust, never privilege. Everything it verifies is `third-party`, even if
the bundle calls itself first-party.

## Deriving the tier

On install the engine tries the distribution key **first**, then the keyring:

- the distribution fingerprint earns `system`;
- a keyring key earns `third-party`, even if it verifies the same bytes;
- the descriptor stores the signer and the tier, and reading a stored `system`
  descriptor re-verifies the stored `manifest.sig` — a hand-edited tier or
  signer is refused;
- a build without a key refuses a `system` descriptor rather than guessing.

An unsigned bundle is accepted only with `[apps] require-signature = false`
(a debug build) and is `third-party` regardless of its id.

### Namespaces

`anth` belongs to the engine and the manifest refuses it for an app.
`anthracite` is first-party: a **keyed** build refuses it from a `third-party`
bundle, and a keyless build — the development build with no `system` tier —
installs the shipped apps as `third-party`. Exports must live under the app's
own package, so the package check covers the interfaces.

## `anth-sign`

The signer is a separate binary in the SDK; the engine never links the signing
half.

```sh
# A new Ed25519 key pair. The private file is 32 bytes of lowercase hex at 0600.
anth-sign keygen --out release.key

# The public half and its fingerprint, without signing anything.
anth-sign public-key --key release.key

# Write <dir>/manifest.sig over manifest.toml + wit/**. The canonical message is
# the engine's own, so the signed set cannot drift from the verified set.
anth-sign <dir> --key release.key
```

`keygen` refuses to overwrite a file. Every command refuses a private key that
group or others can read or write: a key anybody can read is a key anybody can
sign with. The signature is deterministic (Ed25519 over a canonical message), so
signing twice produces the same file.

`scripts/package-app.sh` signs the staging directory before zipping when
`ANTHRACITE_SIGNING_KEY` is set (and `ANTH_SIGN` points at the binary when it is
not on `PATH`). Without the key it prints an explicit unsigned warning; a
release should treat that warning as a failure.

## The release ceremony

`scripts/release.sh --key <private-key>` does the pair that must agree:

1. derives the public half and the fingerprint with `anth-sign public-key`;
2. builds `anth`, `anth-conformance`, `anth-engine` and `anth-panel` with
   `ANTHRACITE_DISTRIBUTION_KEY` set to that public half;
3. boots the freshly built release engine on a throwaway state and stops unless
   `anth key distribution` reports the same fingerprint;
4. builds every app with its own build script, unpacks, signs, repacks and
   verifies it with `anth-conformance --require-signature --key`; a bundle that
   is not `tier system` stops the release;
5. builds and signs the demo emulator bundles the same way (a release build
   refuses the permissive demo configuration);
6. writes `bin/`, `bundles/`, `demo/`, `manifest.json` and `SHA256SUMS`.

Then:

```sh
scripts/release-variants.sh --release <dir>   # core, base, demo, demo-sessions archives
scripts/release-images.sh --variants <dir>    # Docker images of the same variants
```

The variants carry a `README.md`, `manifest.json` and `SHA256SUMS` each; the
images carry the same state volume and entrypoint. See [Installation](installation.md)
and [Demo](demo.md).

## Verifying a download or a bundle

```sh
sha256sum -c SHA256SUMS
./bin/anth --version
./bin/anth --socket state/engine.sock key distribution
anth-conformance --require-signature --key <public> bundle.anth
```

The conformance kit prints `signer` and `tier`; `tier system` means the
distribution key signed it in that build. The release manifest records the key
fingerprint and every digest.

## Rotation and revocation

There is no CRL. Revocation is manual: uninstall the app, remove the key from
the keyring, or rotate the distribution key in a release. Rotation is a release
because the key is compiled in:

1. generate a new pair;
2. build the new binaries with the new public half;
3. re-sign every system bundle, including the shipped apps and the demo emulator;
4. reinstall them; a deployment still running the old binary cannot verify the
   new bundles, which is the point of the tier.

A lost private key means the next release rotates; the old binary keeps trusting
the old key until it is replaced.
