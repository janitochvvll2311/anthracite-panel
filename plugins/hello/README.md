# hello

The example plugin: the smallest thing that is still a real plugin. It answers
one question — what is this host called — through a capability the host grants,
and renders the answer in a small web page. Its greeting is `Hola desde
<hostname>`, Spanish for "Hello from", because the panel's UI targets Spanish.

It exists to be read and copied. Everything a plugin needs is here, and nothing
else is.

## Two independent projects

`backend/` and `frontend/` are separate projects with their own manifests. They
share this directory and a manifest, not a build:

- `backend/` is a Rust `cdylib` for `wasm32-unknown-unknown`. It has no
  dependency on the panel, and `cargo test` in it runs natively.
- `frontend/` is a Bun project. It has its own `package.json` and `bun.lock`.

`plugins/hello` is deliberately absent from `backend/Cargo.toml` and from
`frontend/package.json`. A plugin is shipped to someone else, so it must build
with nothing but its own directory.

## Layout

```
plugin.toml        manifest the host reads: id, alias, version, module digest, capabilities
backend/
  Cargo.toml
  src/logic.rs     pure request handling, testable on the host
  src/lib.rs       the wasm ABI shim: exports anth_alloc, anth_free, anth_handle
frontend/
  src/hello.ts     the web module: a widget implementing the @anth/plugin contract
  src/hello-lit.ts the same, as a Lit element — the contract allows both
  src/hello.test.ts
package.sh         builds both halves into dist/<id>.zip
```

## Build

Needs `rustup target add wasm32-unknown-unknown` once, plus `zip`.

```bash
./package.sh
```

That builds the wasm module, bundles the frontend, writes the module's sha256
into a copy of `plugin.toml`, and produces `dist/org.anth.hello.zip` containing
`plugin.toml`, `plugin.wasm` and `web/hello.js`. It then unpacks that zip into
`plugins/bundles/org.anth.hello/`, which is what a daemon reads:

```bash
ANTH_PLUGINS=$(cd ../bundles && pwd) anth-panel run
```

Point it at `plugins/bundles`, not at `plugins`. The latter holds *sources*: a
registry over it registers `hello/` too, and its manifest still carries the zero
digest by design, so the entry is disabled — and because `resolve` matches an id
before an alias, that disabled `hello` would answer in place of the bundle's
`hello` alias. The digest is computed from the
built module, never hand-written: the host re-hashes the bundle on every load and
disables a plugin whose bytes no longer match, so a stale digest is a plugin that
silently stops working.

## Test

```bash
(cd backend && cargo test)     # runs natively; no wasm toolchain needed
(cd frontend && bun test)
```

The backend keeps its logic in `logic.rs` and its wasm surface in `lib.rs`, so
the tests exercise real request handling without a wasm runtime.

## The ABI

A module exports exactly four functions:

| Export | Purpose |
| --- | --- |
| `anth_abi_version` | returns `1`; the host refuses a module it does not know |
| `anth_alloc` | reserves guest memory for a request |
| `anth_free` | releases it |
| `anth_handle` | takes a request and returns a response |

The host owns the memory: it allocates, writes the request, calls `anth_handle`,
reads the response back and frees both buffers. The module never sees a pointer
it did not receive.

## What this plugin is allowed to do

`plugin.toml` declares one capability, `host.hostname`, with `program =
"hostname"` and a frozen argv. The module supplies *values*, not a program: the
host builds the command line from the grant, and the guest can ask for
`host.hostname` but cannot add a flag to it. A plugin cannot ask for an
arbitrary command, which is what makes installing one a bounded decision rather
than a blank cheque.

Widening that grant is never implicit: the host requires an explicit approval
step, so a plugin update that asks for more is visible before it runs.
