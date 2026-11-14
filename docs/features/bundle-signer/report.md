# Report — Bundle signer

## Summary

`manifest.sig` had a format and a verifier but no producer: the release process
and every third-party author had to invent the canonical message. `anth-sign`
now writes that signature, and `anth-sign keygen` writes a key pair. The tool
reuses the engine's own `bundle::signed_message`, so the signed set cannot drift
from the verified set.

## Delivered

- `sdk/rust/signer`, bin `anth-sign`:
  - `anth-sign <dir> --key <path>` writes `<dir>/manifest.sig`: the 64-byte
    Ed25519 signature over `manifest.toml` plus `wit/**`, standard base64, one
    line. Signing is deterministic and rewrites the file.
  - `anth-sign keygen --out <path>` writes a new 32-byte seed as one line of
    lowercase hex with mode 0600 and refuses to overwrite an existing file.
  - Reading a key refuses a file that group or others can touch (`mode & 0o077`),
    because a key anybody can read is a key anybody can sign with.
  - Both commands print the public key and the fingerprint, which are exactly
    what `anth key add` and `ANTHRACITE_DISTRIBUTION_KEY` consume.
- `bundle::signed_message` and the hex helpers in `anth_compose::signature` are
  public, so the signer calls the one implementation instead of copying it.
- `scripts/package-app.sh` signs the staging directory when
  `ANTHRACITE_SIGNING_KEY` is set (`ANTH_SIGN` overrides the binary path). An
  unsigned package says so on stderr; a missing signer is a hard error rather
  than a silently unsigned release artifact.
- Docs: §7.1 says how the signature is produced, §9.1 documents the tool and the
  key format, §17.3.9 narrows what is still owed to key custody and the release
  job, §10 lists the crate, and the decision log gains `D-Signer`.

## Tests

- `sdk/rust/signer`: **10 passed** (7 library, 3 CLI):
  - `a_signed_bundle_installs_as_system` signs a directory, packs it and runs the
    engine's verifier with the matching distribution key: tier `system`, signer
    fingerprint equal to the generated one.
  - `a_signed_wit_set_cannot_be_edited_afterwards` edits a WIT file after
    signing (a comment, so only the signature can catch it) and the engine
    refuses with `denied`.
  - `a_generated_key_is_private_to_its_owner`, `a_key_others_can_read_is_refused`,
    `keygen_never_overwrites_a_key`, `signing_is_deterministic_and_rewrites_the_signature`,
    `a_directory_without_a_manifest_is_refused`.
- Manual end-to-end with the real `apps/hello` bundle:
  - `anth-sign keygen` → `scripts/package-app.sh apps/hello` with
    `ANTHRACITE_SIGNING_KEY` → the zip contains `manifest.sig`;
  - `anth-conformance --require-signature --key <pub>` passes;
  - the same kit built with `ANTHRACITE_DISTRIBUTION_KEY=<pub>` reports
    `tier system`, and the default build reports `tier third-party`.
- `sdk/rust` workspace, `engine` workspace, `panel/backend`, fmt, clippy and the
  edge gate are green (16 packages).

## Decisions

- **Sign a directory, not a zip.** The bundle is assembled in a staging
  directory by `package-app.sh`; signing there keeps the tool free of zip
  rewriting and guarantees the signature is computed before compression. A
  third-party author signs their directory and zips it afterwards.
- **The engine owns the message, the SDK owns the key.** `anth-sign` calls
  `bundle::signed_message`; nothing about the signing half is linked into
  `anth-engine` or `anth`.
- **The key format is the format the engine already reads**: 32 bytes of
  lowercase hex, and the fingerprint is `sha256(public key)`. No PEM, no second
  encoding to document.
- **`keygen` uses the operating system's randomness** (`getrandom`) instead of a
  deterministic seed, and never overwrites.

## Out of scope

- Signing an existing `.zip` in place; the staging-directory flow covers the
  release and the documented third-party flow.
- Release key custody, the release job that runs `package-app.sh` with the real
  key, and key rotation beyond "it is a release".
- Making the shipped `apps/*` bundles `system` in a release build.
- Namespace ownership (`anth`, `anthracite`) enforced by tier.

## Pending and risks

- A signature covers `manifest.toml` and `wit/**`, not the component: the
  component is covered through its hash in the manifest, which is signed. The
  verifier checks that hash before use, so the chain is intact, but a reader who
  looks only at `manifest.sig` and not at the manifest sees less than they think.
- The private key is a bare file. There is no passphrase, no hardware token and
  no key rotation helper; an operator who loses the key rotates it in a release.
