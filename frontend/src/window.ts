import { LitElement, css, html } from 'lit'
import { customElement, property, state } from 'lit/decorators.js'
import { localized, msg } from '@lit/localize'

/** Fired when the window asks its owner to close it. */
export const WINDOW_CLOSE_EVENT = 'anth-window-close'

interface DragState {
  pointerId: number
  startX: number
  startY: number
  originX: number
  originY: number
}

/**
 * `anth-window`: a floating window over the whole page.
 *
 * It owns the backdrop, the draggable header, the title and the close button;
 * the consumer only fills the default slot with its own content and may add
 * header buttons through the `actions` slot:
 *
 * ```html
 * <anth-window label="Editor" open>
 *   <button slot="actions">Save</button>
 *   <p>Anything goes here.</p>
 * </anth-window>
 * ```
 *
 * The window is fixed to the viewport, so a panel with `overflow` cannot clip
 * it. On compact screens the frame fills the viewport and dragging is off.
 */
@customElement('anth-window')
@localized()
export class AnthWindow extends LitElement {
  @property({ type: Boolean, reflect: true })
  open = false

  /** Title shown in the header and used as the dialog label. */
  @property()
  label = ''

  /** Pixel offset of the frame, relative to its centered position. */
  @state()
  private offset = { x: 0, y: 0 }

  private drag?: DragState

  /** The deep active element before the window opened, restored on close. */
  private restoreFocus?: Element

  private readonly onHostKeydown = (event: KeyboardEvent): void => {
    if (event.key === 'Escape') {
      event.stopPropagation()
      this.close()
    }
  }

  override connectedCallback(): void {
    super.connectedCallback()
    // Slotted content is in the consumer's tree, so a keydown handler in our
    // template would never see it; the host is on the light-DOM path.
    this.addEventListener('keydown', this.onHostKeydown)
  }

  override disconnectedCallback(): void {
    super.disconnectedCallback()
    this.removeEventListener('keydown', this.onHostKeydown)
  }

  override updated(changed: Map<PropertyKey, unknown>): void {
    if (!changed.has('open')) {
      return
    }
    if (this.open) {
      this.restoreFocus = deepActiveElement()
      this.offset = { x: 0, y: 0 }
      void this.updateComplete.then(() => {
        this.querySelector<HTMLElement>('input, textarea, select, button')?.focus()
      })
    } else {
      if (this.restoreFocus instanceof HTMLElement) {
        this.restoreFocus.focus()
      }
      this.restoreFocus = undefined
    }
  }

  show(): void {
    this.open = true
  }

  close(): void {
    if (!this.open) {
      return
    }
    this.open = false
    this.dispatchEvent(
      new CustomEvent(WINDOW_CLOSE_EVENT, { bubbles: true, composed: true }),
    )
  }

  private startDrag(event: PointerEvent): void {
    if (event.button !== 0 || compact()) {
      return
    }
    const target = event.target as HTMLElement | null
    if (target?.closest('button, input, textarea, a')) {
      return
    }
    const handle = event.currentTarget as HTMLElement
    handle.setPointerCapture?.(event.pointerId)
    this.drag = {
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      originX: this.offset.x,
      originY: this.offset.y,
    }
    event.preventDefault()
  }

  private onDrag(event: PointerEvent): void {
    const drag = this.drag
    if (!drag || drag.pointerId !== event.pointerId) {
      return
    }
    const frame = this.shadowRoot?.querySelector<HTMLElement>('.frame')
    if (!frame) {
      return
    }
    // The frame is centered; keep its edges a margin away from the viewport.
    const boundX = Math.max(0, (window.innerWidth - frame.offsetWidth) / 2 - 8)
    const boundY = Math.max(0, (window.innerHeight - frame.offsetHeight) / 2 - 8)
    this.offset = {
      x: clamp(drag.originX + event.clientX - drag.startX, -boundX, boundX),
      y: clamp(drag.originY + event.clientY - drag.startY, -boundY, boundY),
    }
  }

  private endDrag(event: PointerEvent): void {
    if (!this.drag || this.drag.pointerId !== event.pointerId) {
      return
    }
    const handle = event.currentTarget as HTMLElement
    handle.releasePointerCapture?.(event.pointerId)
    this.drag = undefined
  }

  override render() {
    return html`
      <div class="backdrop" @click=${() => this.close()}></div>
      <section
        class="frame"
        role="dialog"
        aria-modal="true"
        aria-label=${this.label}
        style="--window-x: ${this.offset.x}px; --window-y: ${this.offset.y}px"
      >
        <header
          @pointerdown=${this.startDrag}
          @pointermove=${this.onDrag}
          @pointerup=${this.endDrag}
          @pointercancel=${this.endDrag}
        >
          <span class="grip" aria-hidden="true">⠿</span>
          <span class="title">${this.label}</span>
          <div class="actions"><slot name="actions"></slot></div>
          <button type="button" class="close" @click=${() => this.close()}>${msg('Close')}</button>
        </header>
        <div class="body"><slot></slot></div>
      </section>
    `
  }

  static styles = css`
    :host {
      position: fixed;
      inset: 0;
      z-index: 100;
      display: grid;
      place-items: center;
      padding: 12px;
    }
    :host(:not([open])) { display: none; }

    .backdrop {
      position: absolute;
      inset: 0;
      background: rgba(4, 8, 14, 0.62);
    }
    .frame {
      position: relative;
      display: flex;
      flex-direction: column;
      gap: 8px;
      width: min(100%, 56rem);
      height: min(100%, 40rem);
      box-sizing: border-box;
      padding: 12px;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 10px;
      background: var(--panel, #101721);
      box-shadow: 0 18px 48px rgba(0, 0, 0, 0.45);
      transform: translate(var(--window-x, 0px), var(--window-y, 0px));
    }
    header {
      display: flex;
      align-items: center;
      gap: 8px;
      cursor: grab;
      user-select: none;
    }
    header:active { cursor: grabbing; }
    .grip { color: var(--muted, #8b9bad); font-size: 14px; line-height: 1; }
    .title {
      flex: 1 1 auto;
      min-width: 0;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      font: 12px/1.4 var(--mono, monospace);
      color: var(--muted, #8b9bad);
    }
    .actions { display: flex; gap: 6px; }
    .close {
      border: 1px solid var(--panel-border, #223041);
      border-radius: 6px;
      background: rgba(125, 211, 252, 0.08);
      color: var(--text, #dbe6f2);
      padding: 4px 7px;
      font: 12px/1.4 var(--mono, monospace);
      cursor: pointer;
    }
    .close:hover { background: rgba(125, 211, 252, 0.16); }
    .close:focus-visible { outline: 2px solid var(--accent, #7dd3fc); outline-offset: 2px; }
    .body {
      display: flex;
      flex-direction: column;
      flex: 1 1 auto;
      min-height: 0;
    }
    .body > slot { display: contents; }
    ::slotted(*) { min-width: 0; }

    @media (max-width: 48rem) {
      :host { padding: 0; }
      .frame {
        width: 100%;
        height: 100%;
        padding-top: max(12px, env(safe-area-inset-top));
        padding-right: max(12px, env(safe-area-inset-right));
        padding-bottom: max(12px, env(safe-area-inset-bottom));
        padding-left: max(12px, env(safe-area-inset-left));
        border: 0;
        border-radius: 0;
        transform: none;
      }
      header { cursor: default; }
      header:active { cursor: default; }
    }
    @media (pointer: coarse) {
      .close { min-height: 44px; }
    }
  `
}

/** The focused element, crossing open shadow roots. */
function deepActiveElement(): Element | undefined {
  let active: Element | null = document.activeElement
  while (active?.shadowRoot?.activeElement) {
    active = active.shadowRoot.activeElement
  }
  return active ?? undefined
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max)
}

/** True when the shell uses its compact layout, where the window is fullscreen. */
function compact(): boolean {
  return (
    typeof window.matchMedia === 'function' && window.matchMedia('(max-width: 48rem)').matches
  )
}
