# @anth-panel/bun-plugin

A Bun plugin for the Anthracite developer workflow. It resolves the virtual
`anth:client` module generated app code imports in `Bun.build` and the Bun
bundler pipeline.

```toml
# bunfig.toml: register the plugin for builds
preload = ["@anth-panel/bun-plugin"]
```

Or register it explicitly:

```ts
import { anthPlugin } from "@anth-panel/bun-plugin";
Bun.plugin(anthPlugin());
```

The plugin is a build/dev hook, not a `bun anth` subcommand and not a runtime
module resolver; the CLI remains `bunx @anth-panel/cli`.
