# UI reference: the Network section

Status: approved on 2026-10-27 as the visual and structural template for new
feature sections. This document describes the **presentation pattern**, not the
network contract; that one is in [network.md](../network.md).

Use it as the starting point for **Firewall**, **Volumes** and **Accounts**.
Copy the structure and the interaction rules, not the network-specific fields.

## Why this one

The Network section was the first feature after Terminal and Processes that had
to show a live, multi-row inventory with derived values on desktop and phone.
The approved result is deliberately calm: one toolbar, one context strip, one
filter and one card per entity, with no dialog or secondary pane for reading.

The key properties to preserve:

- The feed is visible but never noisy: state, count and one pause control.
- The context (here, the default routes) sits above the list, not inside it.
- Each entity is a self-contained card that reads top to bottom on a phone.
- Cumulative counters become rates only when two samples exist.
- Mutation controls appear only when the daemon's policy flags allow them.

## Anatomy

```text
section

  toolbar        [state] [count]                 [interval] [pause]
  context strip  [label] [chip] [chip]
  filter         [search.........................] [n of m]
  card list      +---------------------------------------------+
                 | accent | name  kind  state                   |
                 |        | MTU  MAC  speed                    |
                 |        | [address] [address]                |
                 |        |                    RX rate / total  |
                 +---------------------------------------------+
```

### 1. Section and tab strip

- One sidebar section per feature; the section id is the route (`#/network`).
- A section that will own more than one view already mounts an
  `anth-tab-panel`, even while it has a single tab. Adding the second tab must
  not restructure the shell.
- The active tab is derived from the route, so back and forward work.
- A parked tab is removed from the light DOM by `anth-tab-panel`; that is what
  stops the component's stream. Do not use `display: none` for hidden tabs.

### 2. Self-contained component

The feature component owns its panel chrome: border, radius, background and
padding. It does not rely on the shell's card wrapper. This keeps the section
usable on its own in tests and makes the toolbar and the list share one visual
surface.

### 3. Toolbar

From left to right:

1. **Feed state** — a small pill with a dot and an uppercase label (`live`,
   `connecting`, `paused`, `stopped`, `error`, `idle`). `role="status"`.
2. **Count** — one line of context, e.g. `2 interfaces · 1 up`. It describes the
   full set, not the filtered set.
3. **Interval** — a `<select>` with `every 1s/2s/5s/10s/30s`. Its accessible
   name is the full `Refresh interval`; the visible text may stay short.
4. **One action button** — pause while live, resume while paused, retry after
   an error or a closed stream. `aria-pressed` reflects the paused state, and
   the label, title and icon change together.

No separate refresh button: a live view already refreshes, and a manual read
while the stream is healthy adds a second source of truth.

### 4. Context strip

A single line of cross-entity facts goes between the toolbar and the filter.
On Network it is the default route, one chip per address family; the empty case
says `none`. On another section it could be the managed firewall table, the
root filesystem, or the current account. It is not a card and does not grow.

### 5. Filter

- A search input with a placeholder that lists what matches: `Filter by
  interface, address, MAC or state`.
- Filtering is local and immediate; the count on the right appears only when
  something is hidden (`1 of 2 interfaces`).
- The toolbar count stays the full-set count.
- An empty result is a normal state with its own sentence, not an error.

### 6. Entity cards

- One card per entity, vertical stack with a small gap.
- A 3px left accent: green when healthy/up, neutral otherwise. The accent
  carries the same meaning as the dot; colour is never the only cue because the
  state badge text is always present.
- **Identity line**: dot, name in mono, kind badge, state badge.
- **Meta line**: small mono facts (`MTU`, MAC, link speed). Facts that do not
  exist are omitted, not rendered as a dash.
- **Address chips**: one chip per value, each with a small family label and an
  optional scope. Chips wrap; long text breaks anywhere.
- **Counters**: two right-aligned groups (`RX`/`TX`), each with the derived rate
  first and the cumulative total second in muted text.
- A problem (errors, drops) is one muted-warning line under the card.

### 6b. Mutations, when the feature has them

The approved Network section later gained mutations without changing the read
anatomy. The pattern for a section with write actions:

- Add **Create** to the toolbar only when the manage policy is on, with an
  `aria-expanded` toggle and one form under the toolbar. The form uses the same
  panel surface as the cards and switches its kind-specific fields.
- Append one full-width **manage row** under the card's read content, separated
  by a top border. It contains the state action (if the control policy is on)
  and edit/rename/delete (if manage is on).
- Disabled actions explain themselves in `title`; do not hide a whole row
  because one target is protected.
- Open one inline form at a time per card for MTU and rename, and one inline
  confirmation for delete. A physical entity adds its stronger warning.
- A mutation refreshes the snapshot and shows one localized success sentence;
  a refusal keeps the last good data and shows the daemon message above the
  list. Do not restart the stream.
- Controls are absent, not disabled, when the policy is off.

### 7. Responsive rule

- Wide: `grid-template-columns: minmax(11rem, 0.9fr) minmax(13rem, 1.1fr) auto`.
- At `44rem` and below: one column; counters move under the identity and align
  left. Nothing scrolls horizontally and no field is hidden.
- The breakpoint is a media query on the component's viewport, which is
  correct for a full-width section. A component that can be narrow inside a
  wide shell (Processes) needs a `ResizeObserver` breakpoint instead.

### 8. States

Every state has a sentence, not a spinner alone:

| State | What the body says |
| --- | --- |
| No client | `No network information yet.` with no toolbar |
| First read | `Reading the interfaces…` |
| Paused | `Paused — press Resume to read the interfaces again.` |
| Error before data | the error text in `role="alert"` |
| Error after data | the error banner above the last good cards |
| Empty filter | `No interfaces match the current filter.` |
| Empty set | the section's own empty sentence |

An error never throws away the last good snapshot.

## Data rules

### Live feed

- The component subscribes on `connectedCallback` and unsubscribes on
  `disconnectedCallback`; returning to the tab reconnects.
- The client exposes `watch(intervalMs, handlers)` and returns the unsubscribe
  function. SSE carries the period in the URL, so changing it opens a new
  stream.
- `onStatus` drives the pill; `onSnapshot` clears a previous error; `onError`
  keeps the last data visible.
- `interval` is a component state, not a global setting. Start at `2s`.

### Derived values

Cumulative counters plus `timestamp_ms` are the daemon's contract. A rate is a
client concern:

```text
rate = (now - previous) / ((now_ms - previous_ms) / 1000)
```

- No previous snapshot, no elapsed time or a counter that went backwards shows
  `—`.
- Pausing or reopening drops the baseline.
- Keep the derivation in `src/lib/<feature>/display.ts` as a pure function;
  test it without a DOM.

### Policy

The snapshot carries the policy flags. The UI reads them and hides or disables
mutations:
- no mutation controls at all while the feature is read-only;
- when mutations land, disable with a `title` that explains *why* instead of
  offering a button that is certain to fail;
- the daemon re-checks every policy and guard. The UI hint is never the policy.

## Code shape

| Piece | Responsibility |
| --- | --- |
| `src/network-client.ts` | Wire types, `get()`, `watch()`, HTTP/SSE parsing and errors |
| `src/lib/network/display.ts` | Pure derived values: rates, formatting, filter, counts |
| `src/network.ts` | Lit element: feed lifecycle, toolbar, states, cards, CSS |
| `src/destinations.ts` | Section id, panel ids, labels, routes and section order |
| `src/app.ts` | Mounts the section and tab panel, injects the client, paints labels |
| `anth-app/src/main.ts` | Constructs the real HTTP client |
| `src/network-display.test.ts` | Pure helpers and edge cases |
| `src/network.test.ts` | Component with a fake client; HTTP client errors |
| `src/shell.test.ts` | Nav entry, route, tab and panel mounting |

## Accessibility and localization

- The toolbar status is `role="status"`; errors are `role="alert"`.
- The icon-only state is never the only label: every button has `aria-label`
  and `title`; the pause button also has `aria-pressed`.
- Focus rings use `outline: 2px solid var(--accent)` with an offset.
- Inputs and buttons are at least 30px high; touch targets on phones are 44px.
- Every user-facing sentence uses `msg()`; interpolated strings use
  ``msg(str`...`)``. Never call `msg()` with a variable.
- Kernel values (`operstate`, address families, interface kinds) stay raw;
  invented labels (`host`, `link`, `global`) are localized.
- Technical units stay conventional (`MiB/s`, `Gbps`, `MTU`).

## Verification checklist

Before calling a section done:

1. `bun run typecheck`, `bun run test`, `bun run build`, `./build-shell.sh`.
2. `cargo fmt --all --check`, clippy with `-D warnings`, workspace tests.
3. Browser at 1280px and 390px: cards, wrapping, counters, no horizontal
   scroll, no Lit warnings.
4. Spanish: nav, tab, toolbar, context strip, filter, empty and error states.
5. Navigate away and back: the stream closes and reconnects; back/forward
   restores the section.
6. Bad interval and daemon error: the message reaches the UI and the last good
   data stays visible.

## Things not to copy

- Do not add a placeholder tab for a feature that does not exist yet. Add the
  second panel when the second feature lands.
- Do not put the feed state in the global shell; it belongs to the panel.
- Do not compute rates on the daemon: it has no interval.
- Do not add a manual refresh button next to a live feed.
- Do not show a field with `—` just to keep a grid aligned; omit it.
