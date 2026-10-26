/**
 * `anth-app`: the shell.
 *
 * The frame is a fixed-height app column: the header keeps its natural height,
 * the body takes the rest, and `main` is the only scroll container. That is what
 * keeps the title bar and the navigation in place while the content moves, and
 * it is the one thing about the layout a test cannot check — happy-dom does not
 * compute layout. The structure it depends on *is* pinned: the header is a
 * sibling of the scroll container rather than inside it.
 *
 * The frame is built once and only the active panel is repainted. A plugin
 * widget is a live element, so a `render()` that rewrote the shadow root would
 * re-create it on every refresh and drop whatever state it held.
 *
 * Navigation, the drawer and the panel tabs all read `destinations.ts`, so a new
 * destination is declared once and appears in the sidebar, the drawer and the
 * tab strip together.
 */

import type { HostInfo } from "@anth/wire";

import { DESTINATIONS, hashFor, idFromHash } from "./destinations";
import { escape } from "./html";
// The shell renders these three, so it imports them: a test that mounts
// `anth-app` on its own must still get working custom elements.
import "./host-info";
import "./maintenance";
import "./fab";
import "./nav-drawer";
import "./tab-panel";
import { DRAWER_CLOSE_EVENT } from "./nav-drawer";
import type { AnthHostInfo } from "./host-info";
import { TAB_CHANGE_EVENT, type TabChangeDetail } from "./tab-panel";

import type { HostInfoClient } from "./host-info-client";
import { DASHBOARD_SLOT, type WidgetHost, slotsFor } from "./widget-slots";

export class AnthApp extends HTMLElement {
  /** Set before mounting by `main`, or replaced by a test. */
  static client: HostInfoClient | undefined;

  /**
   * Where plugin widgets come from, or undefined for a shell with no plugins.
   *
   * One seam rather than two static fields: a catalog without a loader cannot
   * mount anything, and an absent catalog means "no plugins", which is the
   * state a test that only cares about the host card wants.
   */
  static widgets: WidgetHost | undefined;

  #info: HostInfo | undefined;
  #error = "";
  #loading = false;
  #mounted = false;

  #drawerOpen = false;
  #active = DESTINATIONS[0]!.id;

  connectedCallback(): void {
    if (!this.shadowRoot) {
      this.attachShadow({ mode: "open" });
    }
    this.#active = idFromHash(location.hash);
    this.render();
    window.addEventListener("hashchange", this.#onHashChange);
    // The element is connected by `customElements.define` while `app.ts` is
    // evaluated, which is before the composition root in `main.ts` assigns the
    // client. Deferring one microtask lets that assignment happen first, so a
    // mounted element does not report a client that is about to arrive.
    queueMicrotask(() => void this.refresh());
    queueMicrotask(() => void this.mountWidgets());
  }

  disconnectedCallback(): void {
    window.removeEventListener("hashchange", this.#onHashChange);
  }

  async refresh(): Promise<void> {
    const client = AnthApp.client;
    if (!client) {
      this.#error = "No client was configured";
      this.render();
      return;
    }

    this.#loading = true;
    this.#error = "";
    this.render();
    try {
      this.#info = await client.read();
    } catch (error) {
      this.#error = error instanceof Error ? error.message : String(error);
    }
    this.#loading = false;
    this.render();
  }

  private render(): void {
    const root = this.shadowRoot;
    if (!root) {
      return;
    }
    // The frame is written once. A widget mounted into it is a live element, so
    // rewriting the shadow root on a refresh would discard it and its state.
    if (!this.#mounted) {
      root.innerHTML = `
        <style>${STYLE}</style>
        <a class="skip" href="#main" data-skip>Skip to content</a>
        <header>
          <div class="header-main">
            <button class="menu" type="button" aria-label="Open navigation"
                    aria-controls="app-navigation" aria-expanded="false">☰</button>
            <h1>Anthracite Panel</h1>
            <span class="version">v0.2.0</span>
          </div>
        </header>
        <div class="shell">
          <aside class="sidebar">
            <nav aria-label="Sections">${this.nav()}</nav>
          </aside>
          <main id="main" tabindex="-1">
            <anth-tab-panel active="${this.#active}">
              <section data-tab="host" label="Host">
                <section class="card" data-host>
                  <div class="card-head">
                    <span class="label">Host</span>
                    <button type="button" data-refresh>Refresh</button>
                  </div>
                  <anth-host-info></anth-host-info>
                </section>
                <div data-slots></div>
                <anth-fab icon="↻" label="Refresh host information"></anth-fab>
              </section>
              <section data-tab="maintenance" label="Maintenance">
                <anth-maintenance></anth-maintenance>
              </section>
            </anth-tab-panel>
          </main>
        </div>
        <anth-nav-drawer id="app-navigation">
          <nav aria-label="Sections">${this.nav()}</nav>
        </anth-nav-drawer>
      `;
      this.#mounted = true;
      this.#wireFrame();
    }

    // The card is built once with the frame. Only the element's properties are
    // reassigned, so a refresh repaints the snapshot without touching the
    // widget slots or the FAB beside it.
    const panel = root.querySelector<AnthHostInfo>("anth-host-info");
    if (!panel) {
      return;
    }
    // The card refreshes itself from here on. The shell still hands it the
    // snapshot it read, so the first paint has data without waiting for a
    // frame, and the widget's own subscription takes over from there.
    panel.watchUrl = AnthApp.client?.watchUrl;
    panel.info = this.#info;
    panel.error = this.#error;
    panel.loading = this.#loading;
    root.querySelector<HTMLButtonElement>("[data-refresh]")?.toggleAttribute(
      "disabled",
      this.#loading,
    );
    root.querySelector("anth-fab")?.toggleAttribute("disabled", this.#loading);
  }

  /** One set of links, rendered twice: sidebar and drawer. */
  private nav(): string {
    return DESTINATIONS.map(
      (destination) => `
        <a class="nav-item" href="${escape(hashFor(destination))}" data-destination="${escape(destination.id)}"
           ${destination.id === this.#active ? 'data-active aria-current="page"' : ""}>
          <span class="nav-icon" aria-hidden="true">${escape(destination.icon)}</span>
          <span class="nav-label">${escape(destination.label)}</span>
        </a>`,
    ).join("");
  }

  readonly #onHashChange = (): void => {
    const active = idFromHash(location.hash);
    if (active === this.#active) {
      return;
    }
    this.#active = active;
    this.#markActive();
  };

  /**
   * Reflects the route in the tab panel and in both navigation lists.
   *
   * The link lists are repainted rather than the whole frame: a full rewrite
   * would discard the mounted widget.
   */
  #markActive(): void {
    const root = this.shadowRoot;
    if (!root) {
      return;
    }
    root.querySelector("anth-tab-panel")?.setAttribute("active", this.#active);
    for (const link of root.querySelectorAll<HTMLAnchorElement>(".nav-item")) {
      if (link.dataset.destination === this.#active) {
        link.dataset.active = "";
        link.setAttribute("aria-current", "page");
      } else {
        delete link.dataset.active;
        link.removeAttribute("aria-current");
      }
    }
  }

  /** The frame's own listeners, attached once with the frame. */
  #wireFrame(): void {
    const root = this.shadowRoot;
    if (!root) {
      return;
    }
    // Fragment navigation cannot cross a shadow boundary, so `href="#main"`
    // alone would do nothing but rewrite the route to `#main`. Focus is moved
    // by hand; `main` carries `tabindex="-1"` so it can take it.
    root.querySelector("[data-skip]")?.addEventListener("click", (event) => {
      event.preventDefault();
      root.querySelector<HTMLElement>("main")?.focus();
    });
    // Attached once with the frame: the button used to be rebuilt on every
    // refresh, which meant re-registering this listener every time.
    root.querySelector("[data-refresh]")?.addEventListener("click", () => {
      void this.refresh();
    });
    const menu = root.querySelector<HTMLElement>(".menu");
    menu?.addEventListener("click", () => {
      this.#drawerOpen = true;
      this.#syncDrawer(menu);
    });
    root.querySelector("anth-nav-drawer")?.addEventListener(DRAWER_CLOSE_EVENT, () => {
      this.#drawerOpen = false;
      this.#syncDrawer(menu);
    });
    // The fab is the panel's primary action. Its `click` reaches this from
    // inside its own shadow root, which is composed, so one listener on the
    // host covers it.
    root.querySelector("anth-fab")?.addEventListener("click", () => {
      void this.refresh();
    });
    root.querySelector("anth-tab-panel")?.addEventListener(TAB_CHANGE_EVENT, (event) => {
      const { id } = (event as CustomEvent<TabChangeDetail>).detail;
      // The hash stays the source of truth, so back and forward walk tabs.
      location.hash = `#/${id}`;
    });
  }

  #syncDrawer(menu: HTMLElement | null): void {
    this.shadowRoot?.querySelector("anth-nav-drawer")?.toggleAttribute("open", this.#drawerOpen);
    menu?.setAttribute("aria-expanded", String(this.#drawerOpen));
  }

  /**
   * Mounts every plugin that declares the dashboard slot.
   *
   * One plugin failing must not cost the others their place on the page, so a
   * refusal is reported per plugin and the loop continues. A section with no
   * widgets stays out of the frame entirely rather than rendering an empty box.
   */
  private async mountWidgets(): Promise<void> {
    const host = AnthApp.widgets;
    const container = this.shadowRoot?.querySelector<HTMLElement>("[data-slots]");
    if (!host || !container) {
      return;
    }

    let plugins;
    try {
      plugins = await host.catalog.list();
    } catch (error) {
      container.innerHTML = `<p class="error" role="alert">${escape(
        `reading the plugin list failed: ${describe(error)}`,
      )}</p>`;
      return;
    }

    container.innerHTML = "";
    for (const plugin of plugins) {
      for (const slot of slotsFor(plugin, DASHBOARD_SLOT)) {
        const section = document.createElement("section");
        section.className = "card slot";
        section.dataset.plugin = plugin.id;
        section.dataset.slot = slot.slot;
        const heading = document.createElement("div");
        heading.className = "card-head";
        const label = document.createElement("span");
        label.className = "label";
        label.textContent = slot.label ?? plugin.name;
        heading.append(label);
        container.append(section);
        section.append(heading);

        const target = document.createElement("div");
        section.append(target);
        try {
          await host.loader.mount(plugin, slot, target);
        } catch (error) {
          target.innerHTML = `<p class="error" role="alert">${escape(
            describe(error),
          )}</p>`;
        }
      }
    }
  }
}

function describe(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/*
 * These rules only work together:
 *
 * - `height: 100dvh` with a `100svh` fallback plus `overflow: hidden` make the
 *   host the viewport rather than the document.
 * - `flex: 0 0 auto` on the header keeps its natural height.
 * - `min-height: 0` on the body and on `main` is what lets them shrink below
 *   their content. Without it the column grows, the document scrolls, and the
 *   header leaves with it — the exact bug this layout exists to prevent.
 * - `main` is the only scroll container.
 */
const STYLE = `
  :host { display: flex; flex-direction: column; height: 100svh; height: 100dvh;
          overflow: hidden; color: var(--text, #dbe6f2);
          background: transparent; box-sizing: border-box; }

  .skip { position: absolute; top: -10rem; left: 0.5rem; z-index: 30; padding: 0.5rem 0.75rem;
          color: var(--text, #dbe6f2); border: 1px solid var(--accent, #7dd3fc);
          border-radius: 7px; background: var(--panel, #101721);
          font: 600 12px/1.3 var(--mono, monospace); text-decoration: none; }
  .skip:focus { top: 0.5rem; }
  /* iOS shows the skip link on a tap as well as on focus, so it has to be a
     target a finger can hit. */
  @media (pointer: coarse) { .skip { display: inline-flex; align-items: center; min-height: 44px; } }

  header { display: flex; flex: 0 0 auto; flex-wrap: wrap; gap: 1rem; align-items: center;
           justify-content: space-between; padding: 0.9rem clamp(1rem, 4vw, 2.5rem);
           padding-top: max(0.9rem, env(safe-area-inset-top));
           border-bottom: 1px solid var(--panel-border, #223041);
           background: rgba(16, 23, 33, 0.72); }

  .header-main { display: flex; align-items: center; gap: 0.6rem; min-width: 0; }

  h1 { margin: 0; font: 600 16px/1.2 var(--mono, monospace); letter-spacing: 0.02em; }
  .version { color: var(--muted, #8b9bad); font: 12px/1.4 var(--mono, monospace); }

  .menu { display: none; place-items: center; width: 44px; height: 44px; padding: 0;
          color: inherit; border: 1px solid var(--panel-border, #223041); border-radius: 9px;
          background: transparent; font: 400 18px/1 var(--sans, sans-serif); cursor: pointer; }
  .menu:hover, .menu:focus-visible { border-color: var(--accent, #7dd3fc); outline: none; }
  .menu:focus-visible { box-shadow: 0 0 0 2px rgba(125, 211, 252, 0.35); }

  .shell { display: grid; grid-template-columns: clamp(200px, 17vw, 240px) minmax(0, 1fr);
           flex: 1 1 auto; min-height: 0; overflow: hidden; }

  .sidebar { align-self: stretch; height: 100%; min-height: 0; overflow-y: auto;
             overscroll-behavior: contain; padding: 1.1rem 0.75rem;
             padding-left: max(0.75rem, env(safe-area-inset-left));
             border-right: 1px solid var(--panel-border, #223041);
             background: rgba(12, 17, 24, 0.72); }

  .sidebar nav, anth-nav-drawer nav { display: grid; gap: 0.25rem; }

  .nav-item { display: flex; align-items: center; gap: 0.6rem; padding: 0.5rem 0.65rem;
              color: var(--muted, #8b9bad); text-decoration: none;
              border-left: 2px solid transparent; border-radius: 0 8px 8px 0;
              font: 600 12px/1.3 var(--mono, monospace); letter-spacing: 0.02em; }
  .nav-item:hover, .nav-item:focus-visible { color: var(--text, #dbe6f2);
              background: rgba(125, 211, 252, 0.06); outline: none; }
  .nav-item:focus-visible { box-shadow: inset 0 0 0 1px rgba(125, 211, 252, 0.45); }
  .nav-item[data-active] { color: var(--accent, #7dd3fc); border-left-color: var(--accent, #7dd3fc);
              background: rgba(125, 211, 252, 0.1); }
  .nav-icon { width: 1.2rem; text-align: center; font-size: 0.95rem; line-height: 1; }
  .nav-label { flex: 1; }

  main { min-width: 0; min-height: 0; overflow-y: auto; overscroll-behavior: contain;
         -webkit-overflow-scrolling: touch; padding: clamp(1rem, 3vw, 2rem);
         padding-right: max(clamp(1rem, 3vw, 2rem), env(safe-area-inset-right));
         padding-bottom: max(1.5rem, env(safe-area-inset-bottom));
         padding-left: max(clamp(1rem, 3vw, 2rem), env(safe-area-inset-left)); }
  main:focus { outline: none; }
  main > anth-tab-panel { display: block; max-width: 940px; }

  .card { background: rgba(16, 23, 33, 0.86); border: 1px solid var(--panel-border, #223041);
          border-radius: var(--radius, 10px); padding: 1rem 1.15rem; }
  .slot { margin-top: 0.9rem; }
  .card-head { display: flex; align-items: center; justify-content: space-between; margin-bottom: 0.8rem; }
  .label { font-size: 11px; letter-spacing: 0.06em; text-transform: uppercase; color: var(--muted, #8b9bad); }

  button[data-refresh] { font: inherit; min-height: 36px; padding: 4px 12px; border-radius: 6px;
          color: var(--text, #dbe6f2); border: 1px solid var(--panel-border, #223041);
          background: rgba(125, 211, 252, 0.08); cursor: pointer; }
  button[data-refresh]:hover:not(:disabled) { border-color: var(--accent, #7dd3fc); }
  button[data-refresh]:disabled { cursor: progress; opacity: 0.55; }

  dl { display: grid; grid-template-columns: 150px 1fr; gap: 7px 16px; margin: 0; }
  dt { color: var(--muted, #8b9bad); }
  dd { margin: 0; font-variant-numeric: tabular-nums; }

  .error { margin: 0; padding: 10px 12px; border-radius: 8px;
           border: 1px solid rgba(248, 113, 113, 0.4); background: rgba(248, 113, 113, 0.12);
           color: var(--err, #f87171); }

  /* The sidebar becomes a drawer by width, never by orientation.
     A 768px tablet held upright has room for a 200px sidebar and spending it
     on a drawer hides the navigation behind a tap for no gain; a phone turned
     sideways has 844px of width and almost none of height, so orientation
     would bring the sidebar back and leave 300px of column. Width is what
     decides whether two columns fit. */
  @media (max-width: 48rem) {
    .shell { grid-template-columns: minmax(0, 1fr); }
    .sidebar { display: none; }
    .menu { display: grid; }
    header { gap: 0.5rem;
             padding: 0.7rem max(0.8rem, env(safe-area-inset-right)) 0.7rem max(0.8rem, env(safe-area-inset-left));
             padding-top: max(0.7rem, env(safe-area-inset-top)); }
    main { padding: 0.8rem;
           padding-right: max(0.8rem, env(safe-area-inset-right));
           padding-bottom: calc(5rem + env(safe-area-inset-bottom));
           padding-left: max(0.8rem, env(safe-area-inset-left)); }
    dl { grid-template-columns: 1fr; gap: 2px; }
    dt { margin-top: 0.5rem; font-size: 11px; letter-spacing: 0.05em; text-transform: uppercase; }
    /* The 16px floor is Safari's zoom threshold for a focused field; below it,
       tapping an input zooms the page and the layout the visitor sees is not
       the one this stylesheet describes. */
    button[data-refresh] { font-size: max(16px, 1rem); }
  }

  @media (pointer: coarse) {
    button[data-refresh] { min-height: 44px; }
    /* The same 44px on every link in the navigation, including the drawer's:
       the drawer is slotted, so one rule on the host covers both copies. */
    .nav-item { min-height: 44px; }
  }
`;

if (!customElements.get("anth-app")) {
  customElements.define("anth-app", AnthApp);
}
