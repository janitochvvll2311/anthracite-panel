# Sdk Bun

`org.example.sdk-bun` — an Anthracite app.

```sh
anth-sign keygen --out dev.key
cargo anth app build
cargo anth app check
cargo anth app package --key dev.key
cargo anth app verify --public-key <public-key>
```

`cargo anth app build` compiles `sdk-bun` for wasm32-unknown-unknown, wraps it with
`wasm-tools`, and updates the manifest hashes.
The UI lives in `src/main.ts`; `cargo anth app build` runs `bun build` into
`web/app.js` and the manifest pins its hash.

`anth-sign` and `anth-conformance` ship in the GitHub Release `bin/`; put that
directory on `PATH`. The generated tree is standalone; it does not depend on
the Anthracite repository.
