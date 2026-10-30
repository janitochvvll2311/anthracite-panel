# Hello

The smallest reference app. It declares one import (`org.anth.host.info`), two
HTTP anchors and one UI slot, and answers `GET /greeting` from the host snapshot.
It is an ABI 4 wasm guest and a v3 manifest; the build enables the `abi4` feature
of `anth-plugin`.

```bash
./package.sh
ANTH_PLUGIN_KEY=dist/hello-key.pem # optional; otherwise a dev key is generated
```

`package.sh` builds `backend/` for `wasm32-unknown-unknown`, wraps it with
`web/hello.js`, fills both digests, signs `manifest.toml` with Ed25519 and writes
`dist/org.anth.hello.zip`. The public key is printed; set it as
`[apps] public_key` before installing.

The widget is served inside the sandboxed iframe and only uses `api.call`; it
has no network or panel DOM access.
