/**
 * `anth-nav-drawer`: the side navigation for the narrow layout.
 *
 * A thin wrapper over a native `<dialog>`: the top layer, the focus trap,
 * Escape and focus restoration come from the platform rather than from a
 * hand-written keydown handler.
 *
 * The drawer never closes itself. It reports *why* it should close and lets the
 * shell flip `open`, so the shell stays the single source of truth for its own
 * state. The navigation is slotted, which lets the shell reuse the same links,
 * labels and active state as the sidebar.
 */

import { msg } from '@lit/localize'

import './icon'

export const DRAWER_CLOSE_EVENT = 'anth-drawer-close'

export type DrawerCloseReason = 'escape' | 'backdrop' | 'close' | 'link'

export interface DrawerCloseDetail {
  reason: DrawerCloseReason
}

export class AnthNavDrawer extends HTMLElement {
  static get observedAttributes(): string[] {
    return ['open']
  }

  get open(): boolean {
    return this.hasAttribute('open')
  }

  set open(value: boolean) {
    this.toggleAttribute('open', value)
  }

  #mounted = false
  #wired = false

  connectedCallback(): void {
    if (!this.shadowRoot) {
      this.attachShadow({ mode: 'open' })
    }
    if (!this.#mounted) {
      this.#mounted = true
      this.#paint()
    }
    if (!this.#wired) {
      this.#wired = true
      this.addEventListener('click', this.#onHostClick)
      globalThis.addEventListener('lit-localize-status', this.#onLocale)
    }
    this.#sync()
  }

  disconnectedCallback(): void {
    globalThis.removeEventListener('lit-localize-status', this.#onLocale)
    if (this.#wired) {
      this.#wired = false
      this.removeEventListener('click', this.#onHostClick)
    }
  }

  attributeChangedCallback(): void {
    this.#sync()
  }

  readonly #onLocale = (): void => {
    this.#paint()
  }

  #paint(): void {
    const root = this.shadowRoot
    if (!root) {
      return
    }
    root.innerHTML = `
      <style>${STYLE}</style>
      <dialog aria-label="${msg('Navigation')}">
        <div class="head">
          <span class="title">${msg('Navigation')}</span>
          <button class="close" type="button" aria-label="${msg('Close navigation')}"><anth-icon name="close"></anth-icon></button>
        </div>
        <div class="body"><slot></slot></div>
        <div class="foot"><slot name="footer"></slot></div>
      </dialog>
    `
    this.#wireInner()
    this.#sync()
  }

  #wireInner(): void {
    const root = this.shadowRoot
    if (!root) {
      return
    }
    // The paint replaces the shadow tree, so these listeners are re-attached
    // here. The host listener lives in `connectedCallback` and is added once.
    root.querySelector('.close')?.addEventListener('click', () => this.#requestClose('close'))
    root.querySelector('dialog')?.addEventListener('cancel', (event) => {
      // The platform would close the dialog on Escape; keep that decision with
      // the shell so `open` and the dialog cannot disagree.
      event.preventDefault()
      this.#requestClose('escape')
    })
    root.querySelector('dialog')?.addEventListener('click', (event) => {
      // A click on the dialog itself is a click on the backdrop: the panel's
      // own children do not bubble up to the dialog as its target.
      if (event.target === event.currentTarget) {
        this.#requestClose('backdrop')
      }
    })
  }

  #sync(): void {
    const frame = this.shadowRoot?.querySelector('dialog')
    if (!frame) {
      return
    }
    if (this.open && !frame.open) {
      frame.showModal()
    } else if (!this.open && frame.open) {
      frame.close()
    }
  }

  /** A navigation link closes the drawer, since the page behind it changed. */
  readonly #onHostClick = (event: MouseEvent): void => {
    if (event.composedPath().some((node) => node instanceof HTMLAnchorElement)) {
      this.#requestClose('link')
    }
  }

  #requestClose(reason: DrawerCloseReason): void {
    this.dispatchEvent(
      new CustomEvent<DrawerCloseDetail>(DRAWER_CLOSE_EVENT, {
        detail: { reason },
        bubbles: true,
        composed: true,
      }),
    )
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
  .close { display: grid; place-items: center; width: 44px; height: 44px; padding: 0;
           color: inherit; background: transparent; border: 1px solid var(--panel-border, #223041);
           border-radius: 8px; font-size: 15px; cursor: pointer; }
  .close:hover, .close:focus-visible { border-color: var(--accent, #7dd3fc); outline: none; }
  .body { flex: 1 1 auto; min-height: 0; overflow-y: auto; overscroll-behavior: contain;
          padding: 1rem 0.75rem; padding-left: max(0.75rem, env(safe-area-inset-left)); }
  .foot { flex: 0 0 auto; padding: 0.75rem max(1rem, env(safe-area-inset-right))
          max(1rem, env(safe-area-inset-bottom)) max(1rem, env(safe-area-inset-left));
          border-top: 1px solid var(--panel-border, #223041); }
  @media (min-width: 48.0625rem) { dialog { display: none !important; } }
`

if (!customElements.get('anth-nav-drawer')) {
  customElements.define('anth-nav-drawer', AnthNavDrawer)
}
