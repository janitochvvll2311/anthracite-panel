# anth-service

The native service side of Anthracite's private WIT-RPC protocol. It implements
`/health`, `POST /__anth/call`, the broker client and the audit bridge over a
Unix socket.

```rust
anth_service::serve(socket_path, handler).await?;
```

It depends on `anth-error`, not on the engine contract, so a third-party service
can be built and published without the engine.
