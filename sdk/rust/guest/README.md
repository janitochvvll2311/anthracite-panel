# anth-guest

The guest SDK for Anthracite WebAssembly apps: a re-export of `wit-bindgen` used
by an app's generated bindings, plus the host WIT catalog.

```toml
[dependencies]
anth-guest = "0.8"
```

A build script can copy the catalog with `anth_guest::wit_path()`.
