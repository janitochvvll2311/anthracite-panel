import { LitElement, css, html, nothing } from 'lit'
import type { PropertyValues } from 'lit'
import { customElement, property, query } from 'lit/decorators.js'

/**
 * Presentational modal frame.
 *
 * It is a thin wrapper over the native `<dialog>` element, which already
 * provides the top layer, the focus trap, `Escape` handling and focus
 * restoration on close. Reimplementing any of that by hand is how modals end up
 * letting a Tab press reach the page behind them.
 *
 * The component never closes itself. It reports the reason it should close and
 * the owner decides, so a dialog with unsaved input can refuse.
 */

export type ModalCloseReason = 'escape' | 'backdrop' | 'close' | 'programmatic'

export type ModalSize = 'sm' | 'md' | 'lg'

export const ANTH_MODAL_CLOSE_EVENT = 'anth-modal-close'

export interface AnthModalCloseDetail {
  reason: ModalCloseReason
}

@customElement('anth-modal')
export class AnthModal extends LitElement {
  @property({ type: Boolean, reflect: true })
  open = false

  @property()
  heading = ''

  @property({ reflect: true })
  size: ModalSize = 'md'

  /**
   * When set, `Escape` and a backdrop click are ignored. A destructive
   * confirmation that is already running uses this so a stray key press cannot
   * dismiss an operation halfway through.
   */
  @property({ type: Boolean, attribute: 'no-dismiss' })
  noDismiss = false

  @query('dialog')
  private frame?: HTMLDialogElement

  protected override updated(changed: PropertyValues<this>): void {
    if (!changed.has('open')) return
    const frame = this.frame
    if (!frame) return

    if (this.open && !frame.open) {
      frame.showModal()
      this.focusInitial()
    } else if (!this.open && frame.open) {
      frame.close()
    }
  }

  /** A slotted `[autofocus]` target wins; otherwise the frame takes focus. */
  private focusInitial(): void {
    const preferred = this.querySelector<HTMLElement>('[autofocus]')
    ;(preferred ?? this.frame)?.focus()
  }

  private requestClose(reason: ModalCloseReason): void {
    this.dispatchEvent(
      new CustomEvent<AnthModalCloseDetail>(ANTH_MODAL_CLOSE_EVENT, {
        detail: { reason },
        bubbles: true,
        composed: true,
      }),
    )
  }

  private onCancel(event: Event): void {
    // `preventDefault` keeps the native close from racing the owner's state, so
    // `open` stays the single source of truth.
    event.preventDefault()
    if (this.noDismiss) return
    this.requestClose('escape')
  }

  private onFrameClick(event: MouseEvent): void {
    // The frame has no padding, so a click whose target is the frame itself
    // landed on the backdrop.
    if (event.target !== this.frame || this.noDismiss) return
    this.requestClose('backdrop')
  }

  private onCloseClick(): void {
    if (this.noDismiss) return
    this.requestClose('close')
  }

  override render() {
    return html`
      <dialog
        part="frame"
        aria-label=${this.heading || nothing}
        @cancel=${this.onCancel}
        @click=${this.onFrameClick}
      >
        <div class="panel" part="panel">
          <header>
            <h2>${this.heading}</h2>
            ${this.noDismiss
              ? nothing
              : html`<button
                  type="button"
                  class="icon"
                  aria-label="Close"
                  data-action="close"
                  @click=${this.onCloseClick}
                >
                  ×
                </button>`}
          </header>
          <div class="body" part="body"><slot></slot></div>
          <footer part="footer"><slot name="footer"></slot></footer>
        </div>
      </dialog>
    `
  }

  static styles = css`
    :host {
      display: contents;
    }

    dialog {
      padding: 0;
      border: none;
      background: transparent;
      color: var(--text, #dbe6f2);
      max-width: none;
      max-height: none;
    }

    dialog::backdrop {
      background: rgba(3, 7, 12, 0.72);
      backdrop-filter: blur(2px);
    }

    .panel {
      display: flex;
      flex-direction: column;
      gap: 14px;
      width: min(560px, calc(100vw - 2rem));
      padding: 18px 20px 20px;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 10px;
      background: var(--panel, #101721);
      box-shadow: 0 24px 60px rgba(0, 0, 0, 0.45);
      font: 13px/1.5 var(--mono, monospace);
    }

    :host([size='sm']) .panel {
      width: min(400px, calc(100vw - 2rem));
    }

    :host([size='lg']) .panel {
      width: min(840px, calc(100vw - 2rem));
    }

    header {
      display: flex;
      align-items: flex-start;
      gap: 12px;
    }

    h2 {
      flex: 1;
      margin: 0;
      font-size: 15px;
      font-weight: 600;
      letter-spacing: -0.01em;
    }

    .body {
      color: var(--text, #dbe6f2);
    }

    footer {
      display: flex;
    }

    footer ::slotted(*) {
      display: flex;
      gap: 8px;
      margin-left: auto;
    }

    .icon {
      padding: 0 6px;
      border: none;
      border-radius: 6px;
      background: transparent;
      color: var(--muted, #8b9bad);
      font: 16px/1 var(--mono, monospace);
      cursor: pointer;
    }

    .icon:hover {
      color: var(--text, #dbe6f2);
    }

    .icon:focus-visible {
      outline: 2px solid var(--accent, #7dd3fc);
      outline-offset: 2px;
    }
  `
}

declare global {
  interface HTMLElementTagNameMap {
    'anth-modal': AnthModal
  }
}
