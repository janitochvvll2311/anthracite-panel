# Panels

A panel is an application. It links the engine (or talks to it over TCP), serves
a web UI and mounts the contributions of the composed apps. The engine serves no
UI and knows no slot names; `panel.view` and the rest are a panel contract, not a
platform feature. Our official panel is one implementation; the second panel in
`sdk/ts/panel/examples/minimal-panel` is the other.

## The contract

`@anth-panel/panel` owns the vocabulary:

| Export | Meaning |
| --- | --- |
| `VIEW`, `ACTION`, `SETTINGS`, `FAB`, `MODAL` | the portable slot ids |
| `PORTABLE` | all five, in order |
| `REFRESH` | the local action a panel sends to a view after a modal closes |
| `ordered(apps, slot)` | contributions in the contract's deterministic order |
| `ignored(apps, slots)` | contributions for slots this panel does not implement |
| `contributionKey` | stable identity for a mounted pane or window |
| `assetUrl`, `encodeModule` | the engine URL for a module, with a cache-busting hash |
| `mountFrame`, `frameContext`, `byId`, `element`, `button`, `errorCard`, `safeDispose` | the bridge and DOM helpers |

The **surface** is `GET /api/engine/composition`'s `surface`: the running apps,
their slots, CLI verbs, links and the degraded set. A panel declares which slots
it implements and renders those contributions; the rest are reported as ignored,
not as errors.

## The iframe bridge

App modules never run in the panel origin. The panel fetches the module bytes
from `/api/apps/{id}/assets/{module}?v=<hash>` (hash-pinned in the manifest),
verifies them and sends the source to a sandboxed iframe through `postMessage`.
The frame has an opaque origin: it cannot read the panel DOM, cookies or storage,
and every route it calls goes through a scoped RPC that accepts only the app's
own `/api/apps/{id}/` prefix.

```ts
import { assetUrl, mountFrame, ordered } from "@anth-panel/panel";

const [contribution] = ordered(surface.apps, "panel.view");
const host = document.querySelector("#view") as HTMLElement;
const dispose = await mountFrame(host, {
  source: await (await fetch(assetUrl(contribution))).text(),
  context: {
    app: contribution.app,
    contribution: contribution.slot,
    panel: { slots: ["panel.view"], version: "0.7.0" },
    kind: "view",
    params: undefined,
  },
  request: (input, init) => fetch(input, init),
});
```

The module receives `context` with `app`, `contribution`, `panel`, `kind`,
optional `params`, and a scoped `fetch` (and streaming response support). It can
ask the panel to open another of its own views (`openView`) or a modal
(`openModal`), send a local action (`action`), or close itself. `mountFrame`
returns a dispose function; `context.panel.slots` tells the module which slots
the panel implements.

## Assets and hashes

Only a `slot.module` (or a declared icon asset) is servable. The engine re-hashes
it against the manifest on the first request, answers `ETag: "<sha256>"` with
`Cache-Control: immutable`, and serves `304` on a matching `If-None-Match`. A
module whose bytes do not match the manifest is refused. The panel must not
invent an asset path: `assetUrl` builds the only supported one.

## Authentication and the shell

The official panel links the engine and mounts its session router. The browser
authenticates with the `anth_session` cookie; the panel's shell gate reads the
same cookie, so there is one authentication boundary, not two secrets. The
public pages are `/login`, `/setup`, `/health` and the empty app-frame document;
everything else is gated. `GET /panel/config.json` carries the panel's own
configuration.

The panel sets a CSP that forbids framing the shell (`frame-ancestors 'none'`)
and `X-Frame-Options: DENY`; only the app frame is framed, by the panel itself,
with `frame-ancestors 'self'`. It is not an embeddable widget.

A remote panel (another process) uses TCP with the engine's session API. The
minimal panel example does exactly that: it logs in to a loopback panel, forwards
the cookie, and mounts `panel.view` contributions. See
[`sdk/ts/panel/examples/minimal-panel`](../../sdk/ts/panel/examples/minimal-panel/README.md).

## Building a panel

1. Take `@anth-panel/panel` as a dependency (`bun add @anth-panel/panel`; the
   workspace source is `sdk/ts/panel`).
2. Fetch `GET /api/engine/composition` and read `surface.apps`.
3. Pick the slots you implement; render `ordered(apps, slot)`.
4. Mount each contribution with `mountFrame`; pass a `request` adapter if your
   host needs authentication or logging.
5. Dispose frames when a pane closes; `safeDispose` makes a throwing cleanup
   harmless.
6. Test with the SDK's unit tests (`bun test` in `sdk/ts/panel`) and the minimal
   panel's end-to-end.

## Rules

- The engine is the API; a panel is a client. It must not read the state tree,
  the engine's keys or another app's data.
- The module is untrusted: it sees its own app id and its own routes, not the
  panel's cookies or DOM.
- Slot ids are panel-defined. A panel that does not implement `panel.overlay`
  simply reports those contributions as ignored; the app does not fail.
- The contract's ordering is deterministic (order, app id, tag); two panels lay
  out the same surface the same way.
