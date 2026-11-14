# Report — The release ceremony

## Summary

A distribution key with no ceremony is a key nobody uses. `scripts/release.sh`
does the pair that must agree: it compiles the public half into the engine and
the panel, signs every shipped bundle with the private half, and stops if either
side is wrong. The private key stays on the release machine and is never printed.

## Delivered

- `anth-sign public-key --key <path>`: the read-only half of the tool. It derives
  the public key and fingerprint from a private key (refusing a key others can
  read) without signing anything, so the release can name the key before it uses
  it.
- `scripts/release.sh --key <private-key> [--out <dir>] [--apps <a,b,...>]`:
  1. derives the public key and fingerprint;
  2. builds `anth`, `anth-conformance`, `anth-engine` and `anth-panel` with
     `ANTHRACITE_DISTRIBUTION_KEY` set to that public half;
  3. boots the freshly built release engine on a throwaway state and refuses to
     continue unless `anth key distribution` reports the same fingerprint —
     the check that catches "signed with one key, compiled with another";
  4. builds every app with its own build script (`hello` through
     `scripts/build-app.sh` + `package-app.sh`, the system apps through
     `apps/<app>/build.sh`), unpacks the result, signs it with `anth-sign` and
     repacks it;
  5. verifies each bundle with `anth-conformance --require-signature --key` and
     stops unless the kit reports `tier system`;
  6. builds and signs the eight **demo emulator** bundles into `release/demo/`
     (a release build refuses the permissive configuration, so the emulator is
     signed like the system apps and installs as `system`);
  7. writes `release/bin/` with `anth`, `anth-engine` and `anth-panel`,
     `release/bundles/` with the signed bundles, `SHA256SUMS` over both, and
     `release/manifest.json` with the version, the key fingerprint, the binary
     digests and every bundle's id, version and SHA-256 (the `demo` array holds
     the emulator bundles).
- Docs: §9.1 gains the ceremony and the custody rule, §10 lists the scripts,
  §17.3.9 narrows what is still owed to key custody, `AGENTS.md` lists the
  commands.
- `scripts/release-variants.sh --release <signed-dir>` builds the four
  distribution archives from a signed release:
  - `core`: the panel, CLI and engine with an empty composition;
  - `base`: the same binaries plus the eight signed system bundles;
  - `demo`: the same binaries plus the signed emulator bundles, one persistent
    state per run;
  - `demo-sessions`: the demo with one panel and one state per browser session.
  Each archive carries `bin/`, `run.sh`, `README.md`, `manifest.json` and
  `SHA256SUMS`; the script also writes the tarballs, a combined `SHA256SUMS` and
  `RELEASE-NOTES.md`.
- `demo/sessions/` (`anth-demo-sessions`): the session launcher for the fourth
  variant. A cookie maps a browser to its own panel on its own port and state
  directory; the visitor solves a Cloudflare Turnstile challenge, the launcher
  verifies success, action and hostname server-side, signs them in through the
  engine's session endpoint and forwards the cookie, so there is no operator
  password. Sessions expire after `--ttl-seconds` (30 min by default) and the
  launcher deletes their state. Abuse limits: three sessions per address per
  hour, six per minute and sixty per hour globally, ten attempts per minute and
  an hour-long block after five failed challenges; `Origin`/`Referer` is checked
  on the form post. It is demo tooling — loopback by default, no proxy of its
  own, no framing — not a deployment mode.

## Tests

- `sdk/rust/signer`: **12 passed** (8 library, 4 CLI), including
  `key_info_matches_the_key_that_signs` and the `public-key` parser.
- `scripts/release.sh --apps hello` (a fresh key, release profile):
  - the built engine answered `key distribution` with the signing fingerprint;
  - `hello.anth` verified with `--require-signature` and reported `tier system`;
  - `release/bin/`, `release/bundles/`, `manifest.json` and `SHA256SUMS` are
    written, and `sha256sum -c SHA256SUMS` passes.
- Acceptance with the release binaries: the keyed `anth-engine` installed
  `hello.anth` with `require-signature = true` and no keyring, `app show`
  reported `tier system` and the signing fingerprint, `compose --add hello
  --now` restarted and `anth app hello greet` answered
  (`hello, Release (at … ms)`).
- `scripts/release.sh --apps fs`: the native service bundled, signed and
  verified as `system`; `bin/fs` kept its executable bit through unpack/repack;
  the keyed engine installed it without `[services] allow-third-party`, and the
  log had no denial.
- The four variants, extracted from their tarballs and run with short state
  paths:
  - `core`: the panel answers `/health`, `anth status` reports `ready` and zero
    apps;
  - `base`: eight apps installed, all `tier system`, `started 8 app(s), 0
    degraded` (the real services start as root);
  - `demo`: eight emulator apps, `anth app home info` answers from the fixture
    (`Anthracite Demo CPU`), the session state survives the stop;
  - `demo-sessions`: two cookie jars get two panels on two ports with two
    sessions; reloading a jar keeps its session; creating a user in session A
    is invisible in B; `/start` sets both cookies and the child answers
    authenticated; a TTL of 3 s removes the state and closes the port; a
    non-loopback launcher without Turnstile refuses to start; with a local
    verifier a bad token, a wrong action, a wrong hostname and a cross-origin
    post are refused; five failed challenges block the address and the fourth
    session from one address is `429`; SIGTERM kills the launcher and every
    child panel.
- fmt, clippy and the edge gate on the touched workspaces.

## Images

`scripts/release-images.sh --variants <dir>` turns the staged variants into four
Docker-compatible images (docker-archive tarballs) with `umoci` and `skopeo`, no
container runtime or namespace required. The rootfs is a minimal Alpine
userspace (busybox, musl loader, passwd) plus the static release binaries under
`/opt/anthracite`; `state/` is the volume.

| Image | User | Ports | Notes |
| --- | --- | --- | --- |
| `anthracite-core:0.6.0` | 1000 | 8090 | empty composition |
| `anthracite-base:0.6.0` | root | 8090 | real system services; `nft` is not in the image, so firewall routes need a host-provided nft and `NET_ADMIN` |
| `anthracite-demo:0.6.0` | 1000 | 8090 | signed emulator; `run.sh` state per run |
| `anthracite-demo-sessions:0.6.0` | 1000 | 8091, 8100-8199 | Turnstile env required; `ANTHRACITE_PORT_BASE=8100` |

`docker load -i anthracite-0.6.0-core.tar.gz` (or `podman load`), then
`docker run -p 8090:8090 -v anth-core:/opt/anthracite/state anthracite-core:0.6.0`.
For `demo-sessions`, publish the launcher and child range and set the CAPTCHA
variables:
`-p 8091:8091 -p 8100-8199:8100-8199 -e ANTHRACITE_CAPTCHA_SITE_KEY -e ANTHRACITE_CAPTCHA_SECRET -e ANTHRACITE_CAPTCHA_HOSTNAME -e ANTHRACITE_TRUST_PROXY=1`.

**Podman in this environment.** Podman 5.8.10 is installed under `/tools`
(`apk add podman`), but the sandbox blocks `clone(CLONE_NEWUSER)` and mounts
cgroups read-only, so Podman and Buildah cannot run or build containers here
(`cannot clone: Operation not permitted`). The images are produced with the OCI
tooling that does not need namespaces and are Docker/Podman-compatible; they
were validated with `skopeo inspect` and by chrooting the extracted layer
(`anth --version`, the manifest and the variant tree), not by running a
container.

## Decisions

- **One script, two halves, one key.** The dangerous failure is a release whose
  binaries and bundles disagree; the script derives both from the same file and
  proves it by booting the binary it built.
- **The private key is an input, never a repository artifact.** No key material,
  no CI secret and no default path: an operator passes the file, and `anth-sign`
  refuses permissions that let anyone else read it.
- **Bundles are signed after their build, not instead of it.** Each app keeps
  owning how it is assembled; the release unpacks, signs and repacks, so there is
  one signing path for wasm and native-service bundles alike.
- **The emulator is signed too.** A release build refuses `require-signature =
  false` and `allow-third-party = true`, so the demo bundles carry the distribution
  signature and install as `system`; the permissive developer configuration stays
  a debug-only path.
- **Sessions are panels.** Per-browser isolation without engine changes means one
  panel and one state per browser, selected by a cookie from a launcher page; no
  proxy, no cookie rewriting and no engine knowledge of a demo.
- **The manifest is evidence, not configuration.** It records what was signed, so
  a deployment can compare digests; nothing reads it back.

## Out of scope

- Storing or rotating the key pair: custody is an operator decision, and rotation
  is "run the ceremony again with a new pair".
- Publishing bundles to a registry or an app store (§27).
- Replacing any keys on an existing deployment; a release key rotation requires
  reinstalling the bundles signed by the new key.
- Session expiry, cleanup and a single-origin proxy for `demo-sessions`; the
  launcher now expires and cleans sessions, and the proxy is the operator's (the
  demo runs on its own domain, not embedded).

## Pending and risks

- The script builds every app with Bun and Cargo; a release needs both toolchains,
  `zip`/`unzip` and a built `panel/frontend/dist`, and it fails early with a
  message if the frontend is missing. A relative `--key` or `--out` is resolved
  from the directory the operator ran the script in, not from the repository.
- `apps/<app>/dist/` is not used: the release always rebuilds, so a stale bundle
  cannot slip into a release.
- A release key that leaks is a release key that must be rotated; there is no
  CRL, so an old binary keeps trusting the old key until it is replaced.
- Service sockets live under the state directory and Linux caps a Unix socket
  path at 107 bytes, so a deeply nested state directory degrades every native
  app; the variant READMEs say to keep the path short.
- `demo-sessions` runs one panel per browser, so memory scales with sessions and
  cookies are host-scoped: one browser, one session. A public deployment needs a
  proxy and a cleanup policy, which is v0.7 work.
