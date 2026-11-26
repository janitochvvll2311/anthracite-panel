# @anth-panel/wit

The versioned Anthracite host WIT catalog (`anth:env`, `anth:store`,
`anth:error`) used by app builds and by the Bun plugin.

```ts
import { catalog } from "@anth-panel/wit";
```

The catalog is the same set `anth-guest` ships; the package is generated from
`sdk/rust/guest/wit/deps` by `bun run build`.
