/**
 * `anth-nav-drawer`: the side navigation for the portrait layout.
 *
 * A thin wrapper over a native `<dialog>`: the top layer, the focus trap,
 * Escape and focus restoration come from the platform rather than from a
 * hand-written keydown handler.
 *
 * The drawer never closes itself. It reports *why* it should close and lets the
 * shell flip `open`, so the shell stays the single source of truth for its own
 * state — a component that closed itself and a shell that also set `open` would
 * disagree the moment one of them was refactored.
 *
 * The navigation is slotted, which keeps the drawer layout-agnostic and lets the
 * shell reuse the same links, labels and active state as the sidebar.
 */

export const DRAWER_CLOSE_EVENT = "anth-drawer-close";

export type DrawerCloseReason = "escape" | "backdrop" | "close" | "link";

export interface DrawerCloseDetail {
  reason: DrawerCloseReason;
}

export class AnthNavDrawer extends HTMLElement {
  static get observedAttributes(): string[] {
    return ["open"];
  }

  get open(): boolean {
    return this.hasAttribute("open");
  }

  set open(value: boolean) {
    this.toggleAttribute("open", value);
  }

  #mounted = false;

  connectedCallback(): void {
    if (!this.shadowRoot) {
      this.attachShadow({ mode: "open" });
    }
    if (!this.#mounted) {
      this.shadowRoot!.innerHTML = `
        <style>${STYLE}</style>
        <dialog aria-label="Navigation">
          <div class="head">
            <span class="title">Navigation</span>
            <button class="close" type="button" aria-label="Close navigation">✕</button>
          </div>
          <div class="body"><slot></slot></div>
        </dialog>
      `;
      this.#mounted = true;
    }

    this.shadowRoot!.querySelector(".close")?.addEventListener("click", () => this.#requestClose("close"));
    this.shadowRoot!.querySelector("dialog")?.addEventListener("cancel", (event) => {
      // The platform would close the dialog on Escape; keep that decision with
      // the shell so `open` and the dialog cannot disagree.
      event.preventDefault();
      this.#requestClose("escape");
    });

    // A click on the dialog element itself is a click on the backdrop: the
    // panel's own children do not bubble up to the dialog as its target.
    this.shadowRoot!.querySelector("dialog")?.addEventListener("click", (event) => {
      if (event.target === event.currentTarget) {
        this.#requestClose("backdrop");
      }
    });

    this.addEventListener("click", this.#onHostClick);
    this.#sync();
  }

  disconnectedCallback(): void {
    this.removeEventListener("click", this.#onHostClick);
  }

  attributeChangedCallback(): void {
    this.#sync();
  }

  #sync(): void {
    const frame = this.shadowRoot?.querySelector("dialog");
    if (!frame) {
      return;
    }
    if (this.open && !frame.open) {
      frame.showModal();
    } else if (!this.open && frame.open) {
      frame.close();
    }
  }

  /** A navigation link closes the drawer, since the page behind it changed. */
  readonly #onHostClick = (event: MouseEvent): void => {
    if (event.composedPath().some((node) => node instanceof HTMLAnchorElement)) {
      this.#requestClose("link");
    }
  };

  #requestClose(reason: DrawerCloseReason): void {
    this.dispatchEvent(
      new CustomEvent<DrawerCloseDetail>(DRAWER_CLOSE_EVENT, {
        detail: { reason },
        bubbles: true,
        composed: true,
      }),
    );
  }
}

const STYLE = `
  :host { display: contents; }
  dialog { position: fixed; inset: 0 auto 0 0; width: min(84vw, 320px); max-width: 320px;
           height: 100svh; height: 100dvh; max-height: 100dvh; margin: 0; padding: 0;
           color: var(--text, #dbe6f2); border: 0; border-right: 1px solid var(--panel-border, #223041);
           background: var(--panel, #101721); box-shadow: 8px 0 30px rgba(0, 0, 0, 0.45); }
  dialog[open] { display: flex; flex-direction: column; }
  dialog::backdrop { background: rgba(2, 6, 12, 0.62); backdrop-filter: blur(2px); }
  .head { display: flex; flex: 0 0 auto; align-items: center; justify-content: space-between;
          gap: 0.75rem; padding: max(1rem, env(safe-area-inset-top)) max(1rem, env(safe-area-inset-right))
          0.75rem max(1rem, env(safe-area-inset-left)); border-bottom: 1px solid var(--panel-border, #223041); }
  .title { color: var(--muted, #8b9bad); font: 600 11px/1.3 var(--mono, monospace);
           letter-spacing: 0.12em; text-transform: uppercase; }
  .close { display: grid; place-items: center; width: 40px; height: 40px; padding: 0;
           color: inherit; background: transparent; border: 1px solid var(--panel-border, #223041);
           border-radius: 8px; font-size: 15px; cursor: pointer; }
  .close:hover, .close:focus-visible { border-color: var(--accent, #7dd3fc); outline: none; }
  .body { flex: 1 1 auto; min-height: 0; overflow-y: auto; overscroll-behavior: contain;
          padding: 1rem 0.75rem; padding-left: max(0.75rem, env(safe-area-inset-left)); }
  /* The drawer is a portrait affordance; in landscape the sidebar is visible. */
  @media (orientation: landscape) { dialog { display: none !important; } }
`;

if (!customElements.get("anth-nav-drawer")) {
  customElements.define("anth-nav-drawer", AnthNavDrawer);
}
