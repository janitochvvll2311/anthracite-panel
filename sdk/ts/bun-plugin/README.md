# @anth-panel/bun-plugin

A Bun plugin for the Anthracite developer workflow. It resolves the virtual
`anth:client` module generated app code imports and re-exports the real
`@anth-panel/client` package.

```sh
bun add @anth-panel/bun-plugin @anth-panel/client
```

```ts
import { anthPlugin } from "@anth-panel/bun-plugin";

const result = await Bun.build({
  entrypoints: ["./src/main.ts"],
  plugins: [anthPlugin()],
});
```

Bun 1.4.4 does not apply a `bunfig.toml` preload plugin to build resolution, so
the plugin is registered explicitly in the build script. It is not a `bun anth`
subcommand and not a runtime module resolver; the CLI remains
`bunx @anth-panel/cli`.
