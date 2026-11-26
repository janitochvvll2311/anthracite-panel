# Sdk Rust

`org.example.sdk-rust` — an Anthracite app.

```sh
anth-sign keygen --out dev.key
cargo anth app build
cargo anth app check
cargo anth app package --key dev.key
cargo anth app verify --public-key <public-key>
```

`cargo anth app build` compiles `sdk-rust` for wasm32-unknown-unknown, wraps it with
`wasm-tools`, and updates the manifest hashes.
`anth-sign` and `anth-conformance` ship in the GitHub Release `bin/`; put that
directory on `PATH`. The generated tree is standalone; it does not depend on
the Anthracite repository.
