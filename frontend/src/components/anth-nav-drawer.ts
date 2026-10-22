import { LitElement, css, html } from 'lit'
import type { PropertyValues } from 'lit'
import { customElement, property, query } from 'lit/decorators.js'
import { localized, msg } from '@lit/localize'

export type DrawerCloseReason = 'escape' | 'backdrop' | 'close' | 'link'

export const ANTH_DRAWER_CLOSE_EVENT = 'anth-drawer-close'

export interface AnthDrawerCloseDetail {
  reason: DrawerCloseReason
}

/**
 * Side navigation drawer for the portrait layout.
 *
 * It is a thin wrapper over a native `<dialog>`: the top layer, focus trap,
 * Escape key and focus restoration come from the platform, not from a
 * hand-written keydown handler. The component never closes itself; it reports
 * why it should close and the shell flips its `open` property, so the shell
 * state stays the single source of truth.
 *
 * The navigation itself is slotted. That keeps the drawer layout-agnostic and
 * lets the shell reuse the same links and active-state logic as the desktop
 * sidebar.
 */
@customElement('anth-nav-drawer')
@localized()
export class AnthNavDrawer extends LitElement {
  @property({ type: Boolean, reflect: true })
  open = false

  @query('dialog')
  private frame?: HTMLDialogElement

  override connectedCallback(): void {
    super.connectedCallback()
    this.addEventListener('click', this.onHostClick)
  }

  override disconnectedCallback(): void {
    this.removeEventListener('click', this.onHostClick)
    super.disconnectedCallback()
  }

  protected override updated(changed: PropertyValues<this>): void {
    if (!changed.has('open')) return
    const frame = this.frame
    if (!frame) return

    if (this.open && !frame.open) {
      frame.showModal()
    } else if (!this.open && frame.open) {
      frame.close()
    }
  }

  private requestClose(reason: DrawerCloseReason): void {
    this.dispatchEvent(
      new CustomEvent<AnthDrawerCloseDetail>(ANTH_DRAWER_CLOSE_EVENT, {
        detail: { reason },
        bubbles: true,
        composed: true,
      }),
    )
  }

  private onCancel(event: Event): void {
    // Keep the native close from racing the shell's `open`; the shell decides.
    event.preventDefault()
    this.requestClose('escape')
  }

  private onFrameClick(event: MouseEvent): void {
    if (event.target !== this.frame) return
    this.requestClose('backdrop')
  }

  private readonly onHostClick = (event: MouseEvent): void => {
    if (event.composedPath().some((node) => node instanceof HTMLAnchorElement)) {
      this.requestClose('link')
    }
  }

  private onCloseClick(): void {
    this.requestClose('close')
  }

  override render() {
    return html`
      <dialog
        part="frame"
        aria-label=${msg('Navigation')}
        @cancel=${this.onCancel}
        @click=${this.onFrameClick}
      >
        <header class="head">
          <span class="title">${msg('Navigation')}</span>
          <button
            type="button"
            class="close"
            aria-label=${msg('Close navigation')}
            @click=${this.onCloseClick}
          >
            ×
          </button>
        </header>
        <div class="content" part="content">
          <slot></slot>
        </div>
      </dialog>
    `
  }

  static override styles = css`
    :host {
      display: contents;
    }

    dialog {
      position: fixed;
      inset: 0 auto 0 0;
      width: min(84vw, 320px);
      max-width: 320px;
      height: 100svh;
      height: 100dvh;
      max-height: 100svh;
      max-height: 100dvh;
      margin: 0;
      padding: 0;
      color: var(--text, #dbe6f2);
      border: 0;
      border-right: 1px solid var(--panel-border, #223041);
      background: var(--panel, #101721);
      box-shadow: 8px 0 30px rgba(0, 0, 0, 0.45);
    }

    dialog[open] {
      display: flex;
      flex-direction: column;
      animation: slide-in 160ms ease-out;
    }

    dialog::backdrop {
      background: rgba(2, 6, 12, 0.62);
      backdrop-filter: blur(2px);
    }

    .head {
      display: flex;
      flex: 0 0 auto;
      align-items: center;
      justify-content: space-between;
      gap: 0.75rem;
      padding:
        max(1rem, env(safe-area-inset-top))
        max(1rem, env(safe-area-inset-right))
        0.75rem
        max(1rem, env(safe-area-inset-left));
      border-bottom: 1px solid var(--panel-border, #223041);
    }

    .title {
      color: var(--muted, #8b9bad);
      font: 600 11px/1.3 var(--mono, monospace);
      letter-spacing: 0.12em;
      text-transform: uppercase;
    }

    .close {
      display: grid;
      place-items: center;
      width: 40px;
      height: 40px;
      padding: 0;
      color: inherit;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 8px;
      background: transparent;
      font: 400 22px/1 var(--sans, sans-serif);
      cursor: pointer;
    }

    .close:hover,
    .close:focus-visible {
      border-color: rgba(125, 211, 252, 0.55);
      outline: none;
    }

    .close:focus-visible {
      box-shadow: 0 0 0 2px rgba(125, 211, 252, 0.35);
    }

    .content {
      flex: 1 1 auto;
      min-height: 0;
      overflow-y: auto;
      padding:
        0.5rem
        max(0.5rem, env(safe-area-inset-right))
        max(0.75rem, env(safe-area-inset-bottom))
        max(0.5rem, env(safe-area-inset-left));
    }

    @media (orientation: landscape) {
      dialog {
        display: none !important;
      }
    }

    @keyframes slide-in {
      from {
        transform: translateX(-100%);
      }
      to {
        transform: translateX(0);
      }
    }

    @media (prefers-reduced-motion: reduce) {
      dialog[open] {
        animation: none;
      }
    }
  `
}

declare global {
  interface HTMLElementTagNameMap {
    'anth-nav-drawer': AnthNavDrawer
  }
}
