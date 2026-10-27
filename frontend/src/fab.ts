/**
 * `anth-fab`: the floating action button of one panel.
 *
 * The button belongs to the panel, not to the shell. A panel knows whether its
 * primary action is available; a shell would have to ask every panel and would
 * still guess. It also follows from the tab panel's parking: a hidden tab is
 * removed from the document, so only the active panel's FAB is ever in it.
 */

import { escape } from './html'
import './icon'

export class AnthFab extends HTMLElement {
  static get observedAttributes(): string[] {
    return ['icon', 'label', 'disabled']
  }

  /** Icon name rendered inside the circle. Decorative: `label` carries the meaning. */
  get icon(): string {
    return this.getAttribute('icon') ?? ''
  }

  set icon(value: string) {
    this.setAttribute('icon', value)
  }

  /** Accessible name and tooltip. */
  get label(): string {
    return this.getAttribute('label') ?? ''
  }

  set label(value: string) {
    this.setAttribute('label', value)
  }

  get disabled(): boolean {
    return this.hasAttribute('disabled')
  }

  set disabled(value: boolean) {
    this.toggleAttribute('disabled', value)
  }

  connectedCallback(): void {
    if (!this.shadowRoot) {
      this.attachShadow({ mode: 'open' })
    }
    this.#paint()
  }

  attributeChangedCallback(): void {
    this.#paint()
  }

  #paint(): void {
    const root = this.shadowRoot
    if (!root) {
      return
    }
    const label = this.label
    root.innerHTML = `
      <style>${FAB_STYLE}</style>
      <button type="button" aria-label="${escape(label)}" title="${escape(label)}"${this.disabled ? ' disabled' : ''}>
        <anth-icon name="${escape(this.icon)}"></anth-icon>
      </button>
    `
  }
}

export const FAB_STYLE = `
  :host { position: fixed; right: max(1rem, env(safe-area-inset-right));
          bottom: calc(1rem + env(safe-area-inset-bottom)); z-index: 20; }
  :host([hidden]) { display: none; }
  button { display: grid; place-items: center; width: 56px; height: 56px; padding: 0;
           color: #071018; border: 1px solid rgba(125, 211, 252, 0.7); border-radius: 50%;
           background: var(--accent, #7dd3fc); box-shadow: 0 10px 24px rgba(0, 0, 0, 0.42);
           font-size: 24px; cursor: pointer;
           transition: transform 100ms ease, background-color 100ms ease; }
  button:hover:not(:disabled) { background: #a5e3ff; }
  button:active:not(:disabled) { transform: scale(0.96); }
  button:focus-visible { outline: 2px solid var(--text, #dbe6f2); outline-offset: 2px; }
  button:disabled { opacity: 0.5; cursor: progress; }
  @media (prefers-reduced-motion: reduce) { button { transition: none; } }
`

if (!customElements.get('anth-fab')) {
  customElements.define('anth-fab', AnthFab)
}
