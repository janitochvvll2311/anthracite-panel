# `anth-demo-sessions`

One panel and one emulator state per browser session, for the public demo.

- `GET /` shows a landing page with a self-hosted proof-of-work challenge. The
  launcher issues a signed, short-lived challenge; the browser finds a nonce and
  submits it. There is no third-party CAPTCHA and no external script.
- `POST /start` verifies the challenge signature, expiry, address binding,
  difficulty and nonce, rejects replays, checks `Origin`/`Referer`, applies the
  limits below, creates a panel on a private port with a fresh state directory,
  signs in through the engine's session endpoint and forwards the session
  cookie. There is no operator password for the visitor.
- Sessions expire after `--ttl-seconds` (1800 by default); the launcher kills the
  panel, deletes its state and forgets the cookie mapping.
- Child panels use an ephemeral port by default. `--port-base`/`--port-count`
  pin them to a range so a container image can publish it (`ANTHRACITE_PORT_BASE`
  and `ANTHRACITE_PORT_COUNT` in the release image).
- Abuse limits: three sessions per address per hour, six per minute and sixty
  per hour globally, ten attempts per address per minute, and five failed
  challenges in ten minutes block that address for an hour. The session count is
  capped by `--max` (8).
- `SIGINT`/`SIGTERM` stop every child panel. State directories under
  `state/sessions/` are removed on expiry, not on shutdown.

Proof-of-work configuration:

```sh
ANTHRACITE_POW_SECRET=<secret>          # recommended; random per process if unset
ANTHRACITE_POW_DIFFICULTY=18            # default; >=12 for a public launcher
ANTHRACITE_POW_TTL_SECONDS=300          # challenge lifetime
ANTHRACITE_TRUST_PROXY=1                # read CF-Connecting-IP / X-Forwarded-For
```

The operator's proxy must overwrite `X-Forwarded-For` (or send
`CF-Connecting-IP`); otherwise the per-address limits see the proxy's address.
`--pow-difficulty` and `--pow-ttl-seconds` override the environment. On a
loopback launcher a lower difficulty is allowed, which keeps local development
fast; a public launcher requires at least 12 bits.

This is demo tooling, not a deployment mode: it does not proxy, it trusts the
operator's proxy for `Host`/`X-Forwarded-Proto`, and the child panels speak
plain HTTP on loopback.

## Run

```sh
cargo build --release
./target/release/anth-demo-sessions \
    --panel ./target/release/anth-panel \
    --cli ./target/release/anth \
    --bundles ./bundles \
    --state ./state
```
