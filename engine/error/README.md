# anth-error

The error kind and envelope shared by Anthracite services, the engine and the
SDK. It is extracted from the engine contract so `anth-service` can be published
without exposing the full contract.

```rust
use anth_error::{Error, ErrorKind};

let error = Error::unavailable("the provider is not running");
assert_eq!(error.kind(), ErrorKind::Unavailable);
```
