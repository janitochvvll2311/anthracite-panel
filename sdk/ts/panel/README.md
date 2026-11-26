# @anth-panel/panel

The panel contract for Anthracite apps: slot vocabulary, deterministic
contribution order, the iframe bridge, asset URLs, streams and the stable test
ids.

```sh
bun add @anth-panel/panel
```

```ts
import { mountFrame, ordered, VIEW } from "@anth-panel/panel";
```

The package is ESM with declarations. A panel module exports `mount(root,
context)` and the panel calls it for each contribution.
