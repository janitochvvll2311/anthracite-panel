# cargo-anth / anth

Create, build, package, sign and verify an Anthracite app outside this
repository.

```sh
cargo install cargo-anth
cargo anth app new org.example.notes --lang rust
cd notes
anth-sign keygen --out dev.key
cargo anth app build
cargo anth app check
cargo anth app package --key dev.key
cargo anth app verify --public-key "$(anth-sign public-key --key dev.key | awk '/^public_key/ {print $2}')"
```

`anth-sign` and `anth-conformance` ship in the GitHub Release `bin/`; put that
directory on `PATH` (or build `anth-signer`/`anth-conformance` from the
workspace). `cargo install cargo-anth` provides `cargo anth`, not those tools.

The same commands are available from the `anth` binary shipped with the
Anthracite release:

```sh
anth app new org.example.notes --lang bun
cd notes
anth app build
anth app package
```

`cargo anth` uses cargo's `cargo-<subcommand>` discovery: it is the same
mechanism as `cargo clippy` and `cargo nextest`. The generated project is
standalone and never references this repository.

## Commands

| Command | What it does |
| --- | --- |
| `app new <id> [--lang rust\|bun] [--dir <path>]` | write a standalone project |
| `app build [--project <dir>]` | cargo + wasm-tools (and `bun build` for `lang = "bun"`), update hashes |
| `app package [--project <dir>] [--key <path>]` | build and write `dist/<id>-<version>.anth` |
| `app sign [--project <dir>] [--key <path>]` | sign an existing bundle |
| `app verify [--project <dir>] [--public-key <hex>]` | run `anth-conformance` |
| `app check [--project <dir>]` | package and run the unsigned conformance check |

`anth.toml` at the project root drives all of them; `--project` may point at the
directory or the `anth.toml` itself.

## Environment

`CARGO`, `BUN`, `WASM_TOOLS`, `ANTH_SIGN` and `ANTH_CONFORMANCE` override the
tools when they are not on `PATH`.
