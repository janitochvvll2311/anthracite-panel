# Hello

The smallest reference app. It declares one capability (`host.hostname`), two
HTTP anchors and one UI slot, and answers `GET /greeting` by running the frozen
`hostname` argv.

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
