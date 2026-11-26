# @anth-panel/cli

Run the Anthracite app developer CLI from Bun:

```sh
bunx @anth-panel/cli app new org.example.notes --lang bun
bunx @anth-panel/cli app build
```

The wrapper executes the `anth` binary from the release `bin/` (or `ANTH_BIN`),
with `cargo anth` as the fallback. It is the Drizzle-style integration: there
is no native `bun anth` subcommand.
