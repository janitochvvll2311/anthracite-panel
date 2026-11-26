# anth-bundle

The canonical Anthracite bundle primitives: the signed message, the WIT-set
hash, SHA-256 helpers and Ed25519 verification.

`anth-sign` and the engine use this crate, so a signature produced by the SDK is
the one the engine verifies.

```rust
let message = anth_bundle::signed_message(std::path::Path::new("app"))?;
anth_bundle::verify(&message, &signature, &public_key)?;
```
