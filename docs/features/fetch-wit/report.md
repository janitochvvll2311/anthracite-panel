# Report — Provider WIT fetch

## Summary

`anth app fetch-wit <id> [--out <dir>]` fetches an installed app's WIT package
from the engine and writes it to a local dependency directory. It removes the
need to vendor a provider's WIT by hand before building a consumer.

## Delivered

- `GET /api/engine/apps/{id}/wit` returns `{app, package, files}` where `files`
  is a `{name: text}` map of the app's `wit/*.wit`.
- `Registry::wit_files` reads the installed package and refuses a missing or
  non-UTF-8 WIT tree.
- `anth app fetch-wit <id>` writes to `wit/deps/<package>` in the source tree
  by default, with `--out <dir>` for an explicit destination. The managed
  default removes stale `.wit` files; an explicit directory is left untouched.
  Package characters that are not path-safe become `_`; file names must be safe
  `.wit` names and writes do not follow symlinks.
- The command validates the app id before the request and prints the directory
  it wrote.

## Tests

- `engine/api`: the WIT endpoint serves `example:notes`, and a missing app is
  `404 not-found`; a corrupt installed tree is an internal error, not a `400`.
- `sdk/rust`: parse test for `fetch-wit --out`, a fake-engine test that asserts
  the requested URL and the written `notes.wit`, and unsafe-name, stale-file
  and symlink refusals.
- Live smoke: fetched `org.anthracite.fs` from the running panel into an explicit
  `--out` directory and into the default `wit/deps/anthracite_fs/`.
- `cargo fmt --check` and `clippy -D warnings` on both touched workspaces.

## Decisions

- The endpoint carries text files, not a zip. WIT is UTF-8, the response is
  inspectable, and the CLI needs no archive dependency.
- The default directory is `wit/deps/<package>` because that is where the guest
  catalog and `wasm-tools` already look for dependency packages.
- Fetching overwrites files in the target directory. The operator chose the
  directory; the engine's package is the source of truth.
- The command fetches an **installed** app's WIT. It is not a remote registry and
  does not resolve versions: the operator installs the provider first.

## Out of scope

- A registry or version solver for provider packages.
- Fetching a bundle that is not installed.
- Writing into `sdk/rust/guest/wit/deps` automatically; that catalog is the
  SDK's own.
- Offline signature verification; the engine already verified the bundle at
  install time.

## Pending and risks

- If two providers share a package name, the last fetch wins. The package
  namespace is expected to be unique; a future registry would make that
  explicit.
- A fetched package is a build input, not authority. The consumer still needs a
  grant for the interfaces it imports.
