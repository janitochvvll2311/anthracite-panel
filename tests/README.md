# End-to-end tests

Bun project with Playwright. The runner starts **demo mode** without `sudo`,
disables auth for the local demo config, runs the browser tests against Vite and
stops both servers on exit.

```bash
cd tests
bun install
./run.sh
```

On this environment Playwright uses `/usr/bin/chromium`. Elsewhere use
`CHROMIUM_PATH=/path/to/chromium` or run `bun run browsers` first.

The tests assume the demo backend, not a real host. They create and delete a
virtual `dummy` interface and add a firewall rule through the UI; both live in
the in-memory state and disappear when the demo stops.
