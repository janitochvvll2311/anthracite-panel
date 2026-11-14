# `anth-demo-sessions`

One panel and one emulator state per browser session, for the public demo.

- `GET /` shows a landing page with a Cloudflare Turnstile widget. A launcher
  that is not loopback (or sets `--public-host`) refuses to start without
  Turnstile and the expected hostname.
- `POST /start` verifies the token server-side (success, `action` and
  `hostname`), checks `Origin`/`Referer`, applies the limits below, creates a
  panel on a private port with a fresh state directory, signs in through the
  engine's session endpoint and forwards the session cookie. There is no
  operator password for the visitor.
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

Turnstile configuration:

```sh
ANTHRACITE_CAPTCHA_SITE_KEY=<site key>      # public; can also be --captcha-site-key
ANTHRACITE_CAPTCHA_SECRET=<secret>          # env only, never argv
ANTHRACITE_CAPTCHA_HOSTNAME=demo.example    # required for a public launcher
ANTHRACITE_TRUST_PROXY=1                    # read CF-Connecting-IP / X-Forwarded-For
```

The operator's proxy must overwrite `X-Forwarded-For` (or send
`CF-Connecting-IP`); otherwise the per-address limits see the proxy's address.
`--captcha-verify-url` overrides the Turnstile endpoint for a local test server.
On a loopback launcher Turnstile is optional, which is what makes local
development work without keys.

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
