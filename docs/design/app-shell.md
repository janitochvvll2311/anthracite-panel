# Web app shell

The shell is a header, a navigation sidebar grouped by theme and a workspace
whose active theme is a tab strip of its panels. Landscape uses the sidebar;
portrait replaces it with a drawer and panel-owned floating action buttons.

## Layout

```
Landscape / desktop

┌──────────────────────────────────────────────────────────────┐
│ header: brand · host control · user · language · sign out    │
├─────────────────┬────────────────────────────────────────────┤
│ Home            │  theme title + description                 │
│                 │  [ Host info ] [ Processes ]               │
│ System          │  ┌──────────────────────────────────────┐  │
│ Storage         │  │ active panel                         │  │
│ Network         │  │                                      │  │
│ Access          │  └──────────────────────────────────────┘  │
│ Extensions      │                                            │
└─────────────────┴────────────────────────────────────────────┘
```

```
Portrait / mobile

┌──────────────────────────────┐
│ ☰  Anthracite Panel  admin ⎋ │
├──────────────────────────────┤
│ System                       │
│ [ Host info ] [ Processes ]  │
│ ┌──────────────────────────┐ │
│ │ active panel             │ │
│ │                     (＋) │ │  panel FAB
│ └──────────────────────────┘ │
└──────────────────────────────┘
```

The header is deliberately the original one. The redesign is the navigation
below it, not a new identity.

The shell is a fixed-height app frame: `anth-app` is `100dvh` (with a `100svh`
fallback) and `overflow: hidden`. The header keeps its natural height, `.shell`
takes the rest, the sidebar stretches to the full column height, and `main` is
the only scroll container. This keeps the navigation visible and makes the
sidebar background and border cover the whole available vertical space.

## Orientation breakpoint

There is no width breakpoint. The base layout is landscape/desktop and a single
`@media (orientation: portrait)` switches to the mobile layout:

- sidebar hidden, menu button visible;
- drawer navigation and panel FABs are used;
- Home cards collapse to one column and the main padding shrinks;
- the header drops “host control” and the visible “Language” label, keeping the
  user, sign out and language selector.

A phone in landscape therefore sees the desktop sidebar, and a tall narrow
desktop window sees the mobile layout. That is intentional: it makes manual
testing a matter of rotating the viewport.

Safe areas are handled with `viewport-fit=cover` and
`env(safe-area-inset-*)` on the header, main, drawer and FAB.

## Themes

| Theme | Short label | Panels | Landing panel |
| --- | --- | --- | --- |
| System | System | Host info, Processes | last used, else Host info |
| Storage | Data | Files, Storage | last used, else Files |
| Network | Network | Network, Firewall | last used, else Network |
| Access | Access | Accounts | Accounts |
| Extensions | Extras | Plugins, Terminal | last used, else Plugins |

The short labels are used in the portrait drawer, where full titles do not fit;
the full title remains the link's `aria-label` and `title`. The landscape
sidebar uses the full titles.

`Home` is not a theme. It is the `#/` page with one card per theme and the
`dashboard.widget` plugin slot. The first card link and the sidebar link for a
theme point at that theme's remembered panel, so returning to a theme resumes
the last tab.

The registry lives in `frontend/src/app/panels.ts`: each `PanelDefinition` has a
`theme`, and `THEMES`, `panelsIn` and `themeForPanel` derive the sidebar and tab
groups. A panel belongs to exactly one theme.

## Routes

| Route | Result |
| --- | --- |
| `#/` | Home |
| `#/panel/<id>` | The theme that owns `<id>`, with that panel's tab active |
| `#/panel/plugin:<plugin-id>:<tag>` | A plugin tab inside the theme named by its anchor slot |
| `#/window/<id>` | Unknown page, intentionally |
| anything else | Unknown page |

`#/panel/<id>` stays canonical so existing deep links and bookmarks keep
working. A tab click writes the same kind of hash, which means back/forward walk
through tabs as real history. The sidebar links a theme to its remembered
panel; the route remains the source of truth for which tab is visible.

`#/window/<id>` is no longer handled. The desktop, its window component,
`windowUrl()` and the router's `window` kind are kept as utilities for a later
window mode; keeping the parser and URL helper does not make the route work.

## Components and state

- **`anth-app`** owns the session gate, the header, the sidebar, the drawer
  state, Home, the theme cache and the hash listener.
- **`anth-nav-drawer`** is a native `<dialog>` side panel. The platform supplies
  the top layer, focus trap, Escape handling and focus restoration. It never
  closes itself: it emits `anth-drawer-close` with `escape`, `backdrop`,
  `close` or `link`, and the shell clears `open`.
- **`anth-theme-page`** owns one theme: its heading, its `anth-tab-panel` and the
  feature elements. It builds the tab panel once and then only changes `active`.
  A tab selection emits `anth-panel-select`; the shell turns that into a hash.
- **Plugin tabs**: a theme may name a `pluginSlot` (`network` uses
  `panel.network.tab`). The shell appends one tab per anchor returned by
  `anchorsBySlot`; the anchor's `label` is the tab title, falling back to the
  plugin name. `anth-plugin-anchor` imports that one module and appends its tag.
  A tab id is `plugin:<plugin-id>:<tag>`, which makes the tab linkable at
  `#/panel/plugin:<plugin-id>:<tag>` and keeps back/forward and theme memory
  working. Installing a plugin emits `anth-plugins-changed`; the shell reloads
  the catalogue, so the Network tab appears without a page reload.
- **`anth-tab-panel`** parks a hidden tab by removing it from the light DOM, not
  by hiding it. A parked panel's SSE stream or terminal WebSocket closes, while
  the element and its state stay in the tab panel's registry.
- **`anth-fab`** is the 56px action button owned by one panel. The panel decides
  whether the action is available, so hidden tabs and read-only daemons cannot
  show an unusable button. It is hidden in landscape with
  `@media (orientation: landscape)`, and positioned above the bottom safe area.
- **`lib/ui/theme-memory`** stores the last panel per theme in `localStorage`
  (`anth.theme.<theme>`). It is only a hint for links; a stale id falls back to
  the first panel.
- **Theme cache**: `anth-app` keeps one `anth-theme-page` per theme. Switching
  themes detaches the page rather than destroying it, so returning restores the
  same tab and panel element. The detached subtree disconnects, which is what
  closes its streams.

A locale change clears the theme cache so theme titles and tab labels rebuild
from the new locale; the route selects the same panel again.

Home keeps the plugin widgets that used to live on the old card page. A plugin
installed from the plugin manager is not reflected in the shell catalogue until
the next session load; that limitation predates this layout.

## Panel FABs

| Panel | Action | Available when |
| --- | --- | --- |
| Host info | Refresh | always |
| Processes | Start process | `spawn_enabled` |
| Files | Upload | `write_enabled` |
| Storage | Refresh | always |
| Network | Create interface | `manage_enabled` |
| Firewall | Add rule | `control_enabled` |
| Accounts | New user / New group | `control_enabled` |
| Plugins | Install plugin | `enabled && install_from_web` |
| Terminal | none | — |

Each FAB calls the same method as the panel's toolbar action; it does not
duplicate the request logic. A component with no available primary action does
not render a FAB at all.

## Accessibility

- The sidebar is a `<nav>` with `aria-current="page"` on the active entry.
- The drawer is a modal `<dialog>`; Escape, focus trapping and focus
  restoration come from the platform.
- FABs have an `aria-label` and `title`; their icon is `aria-hidden`.
- Home and theme icons are `aria-hidden`; the labels carry the meaning.
- The tab strip is an ARIA tablist with arrow/Home/End support from
  `anth-tab-panel`.
- A skip link focuses `<main>` without changing the hash.

## Testing

- `frontend/src/app/anth-app.test.ts` covers the sidebar, Home, deep links, tab
  navigation, plugin tabs, theme memory, the theme-page cache, the drawer
  open/close paths, the broken pop-out route, unknown routes, login and locale.
- `frontend/src/components/anth-theme-page.test.ts` covers one tab per panel,
  detaching the inactive panel, the selection event, external `active` changes,
  building each panel once and the fallback for an unknown id.
- `frontend/src/components/anth-nav-drawer.test.ts` covers the native dialog,
  close reasons and the owner-controlled `open`.
- `frontend/src/components/anth-fab.test.ts` covers label, disabled state,
  click forwarding and the `mobile-only` opt-out.
- `frontend/src/components/anth-fab-integration.test.ts` checks that each panel
  shows or hides its FAB according to the daemon state.
- `frontend/src/lib/ui/theme-memory.test.ts` covers the storage key and blocked
  storage.
- `anth-theme-page`, `anth-nav-drawer` and `anth-fab` have dev harnesses linked
  from `component-index.test.html`; the plugin anchor has a component test.

CSS orientation is not evaluated by happy-dom, so the final visual check is
manual: 390×844 for portrait and 844×390 for landscape.
