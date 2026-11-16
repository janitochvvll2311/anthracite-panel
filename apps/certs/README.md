# Certificates

`org.anthracite.certs` — one inventory of certificates with expiry and chain
state, a managed store, and an atomic publication directory for the proxy.

- Scans `/etc/ssl/certs` and `/etc/letsencrypt/live` (read-only roots) and the
  managed store under `{data}/certs`.
- Imports a PEM leaf/chain/key only after the key matches the leaf; a mismatch
  writes nothing.
- Publishes to `{shared}/tls/<id>/` with `fullchain.pem`, `privkey.pem` and
  `meta.json` at `0600`. The private key is never returned by WIT, a route, the
  log or the panel.
- Only `source = acme` certificates can be marked `auto-renew`.
- Removing a certificate deletes the managed copy and keeps the published one
  until the proxy reloads.

Build:

```sh
apps/certs/build.sh
```
