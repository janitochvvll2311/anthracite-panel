import { LitElement, css, html } from 'lit'
import { customElement, property } from 'lit/decorators.js'

/**
 * A floating action button for the portrait layout.
 *
 * It belongs to one panel, not to the shell: the panel knows whether its
 * primary action is available, and a parked tab keeps its FAB attached to the
 * detached subtree, so only the active panel's FAB is in the document.
 *
 * `mobile-only` is reflected so landscape can hide the button with pure CSS.
 * On landscape the shell uses the sidebar, and a FAB would only cover content.
 */
@customElement('anth-fab')
export class AnthFab extends LitElement {
  /** Glyph shown in the circle. Decorative; `label` carries the meaning. */
  @property()
  icon = ''

  /** Accessible name and tooltip. */
  @property()
  label = ''

  @property({ type: Boolean, reflect: true })
  disabled = false

  @property({ type: Boolean, attribute: 'mobile-only', reflect: true })
  mobileOnly = true

  override render() {
    return html`
      <button
        type="button"
        aria-label=${this.label}
        title=${this.label}
        ?disabled=${this.disabled}
      >
        <span aria-hidden="true">${this.icon}</span>
      </button>
    `
  }

  static override styles = css`
    :host {
      position: fixed;
      right: max(1rem, env(safe-area-inset-right));
      bottom: calc(1rem + env(safe-area-inset-bottom));
      z-index: 20;
    }

    @media (orientation: landscape) {
      :host([mobile-only]) {
        display: none;
      }
    }

    button {
      display: grid;
      place-items: center;
      width: 56px;
      height: 56px;
      padding: 0;
      color: #071018;
      border: 1px solid rgba(125, 211, 252, 0.7);
      border-radius: 50%;
      background: var(--accent, #7dd3fc);
      box-shadow: 0 10px 24px rgba(0, 0, 0, 0.42);
      font: 600 24px/1 var(--mono, ui-monospace, monospace);
      cursor: pointer;
      transition:
        transform 100ms ease,
        background-color 100ms ease;
    }

    button:hover:not(:disabled) {
      background: #a5e3ff;
    }

    button:active:not(:disabled) {
      transform: scale(0.96);
    }

    button:focus-visible {
      outline: 2px solid var(--text, #dbe6f2);
      outline-offset: 2px;
    }

    button:disabled {
      opacity: 0.5;
      cursor: not-allowed;
    }

    @media (prefers-reduced-motion: reduce) {
      button {
        transition: none;
      }
    }
  `
}

declare global {
  interface HTMLElementTagNameMap {
    'anth-fab': AnthFab
  }
}
