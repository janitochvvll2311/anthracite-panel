import { LitElement, css, html, nothing } from 'lit'
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

interface DockItem {
  id: number
  title: string
  restore: () => void
  close: () => void
}

/**
 * `anth-window`: a floating window over the whole page.
 *
 * It owns the backdrop, the draggable header, the title, the minimize,
 * maximize and close buttons; the consumer only fills the default slot with
 * its own content and may add header buttons through the `actions` slot:
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
 * Minimizing parks it in the dock bar at the bottom of the page; every window
 * shares that bar.
 */
@customElement('anth-window')
@localized()
export class AnthWindow extends LitElement {
  @property({ type: Boolean, reflect: true })
  open = false

  /** Title shown in the header and used as the dialog label. */
  @property()
  label = ''

  /** Fills the viewport instead of the normal frame size. */
  @property({ type: Boolean, reflect: true })
  maximized = false

  /** Hidden while its chip sits in the dock bar. */
  @property({ type: Boolean, reflect: true })
  minimized = false

  /** Pixel offset of the frame, relative to its centered position. */
  @state()
  private offset = { x: 0, y: 0 }

  private readonly key = nextWindowId()

  private drag?: DragState

  private docked = false

  /** The deep active element before the window opened, restored on close. */
  private restoreFocus?: Element

  private readonly onHostKeydown = (event: KeyboardEvent): void => {
    if (event.key === 'Escape') {
      event.stopPropagation()
      this.close()
    }
  }

  private readonly raise = (): void => {
    topZ += 1
    this.style.zIndex = String(topZ)
  }

  override connectedCallback(): void {
    super.connectedCallback()
    // Slotted content is in the consumer's tree, so a keydown handler in our
    // template would never see it; the host is on the light-DOM path.
    this.addEventListener('keydown', this.onHostKeydown)
    this.addEventListener('pointerdown', this.raise)
    this.raise()
  }

  override disconnectedCallback(): void {
    super.disconnectedCallback()
    this.removeEventListener('keydown', this.onHostKeydown)
    this.removeEventListener('pointerdown', this.raise)
    this.undock()
  }

  override updated(changed: Map<PropertyKey, unknown>): void {
    if (!changed.has('open')) {
      return
    }
    if (this.open) {
      this.raise()
      this.restoreFocus = deepActiveElement()
      this.offset = { x: 0, y: 0 }
      void this.updateComplete.then(() => this.focusFirst())
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
    this.undock()
    this.minimized = false
    this.maximized = false
    this.open = false
    this.dispatchEvent(
      new CustomEvent(WINDOW_CLOSE_EVENT, { bubbles: true, composed: true }),
    )
  }

  minimize(): void {
    if (!this.open || this.minimized) {
      return
    }
    this.drag = undefined
    this.minimized = true
    this.docked = true
    ensureDock().add({
      id: this.key,
      title: this.label || msg('Window'),
      restore: () => this.restore(),
      close: () => this.close(),
    })
  }

  restore(): void {
    if (!this.minimized) {
      return
    }
    this.undock()
    this.minimized = false
    this.raise()
    void this.updateComplete.then(() => this.focusFirst())
  }

  toggleMaximize(): void {
    if (this.minimized) {
      return
    }
    this.maximized = !this.maximized
    this.offset = { x: 0, y: 0 }
    this.drag = undefined
  }

  private undock(): void {
    if (!this.docked) {
      return
    }
    this.docked = false
    existingDock()?.drop(this.key)
  }

  private focusFirst(): void {
    this.querySelector<HTMLElement>('input, textarea, select, button, [tabindex]')?.focus()
  }

  private startDrag(event: PointerEvent): void {
    if (event.button !== 0 || compact() || this.maximized) {
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
          <span class="grip" aria-hidden="true"></span>
          <span class="title">${this.label}</span>
          <div class="actions"><slot name="actions"></slot></div>
          <button
            type="button"
            class="control control-min"
            title=${msg('Minimize')}
            aria-label=${msg('Minimize')}
            @click=${() => this.minimize()}
          >
            <span class="icon icon-min" aria-hidden="true"></span>
          </button>
          <button
            type="button"
            class="control control-max"
            title=${this.maximized ? msg('Restore') : msg('Maximize')}
            aria-label=${this.maximized ? msg('Restore') : msg('Maximize')}
            @click=${() => this.toggleMaximize()}
          >
            <span class="icon ${this.maximized ? 'icon-restore' : 'icon-max'}" aria-hidden="true"></span>
          </button>
          <button
            type="button"
            class="control close"
            title=${msg('Close')}
            aria-label=${msg('Close')}
            @click=${() => this.close()}
          >
            <span class="icon icon-close" aria-hidden="true"></span>
          </button>
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
    :host(:not([open])),
    :host([minimized]) { display: none; }
    :host([maximized]) { padding: 0; }

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
      min-width: 0;
      max-width: 100%;
      box-sizing: border-box;
      padding: 12px;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 10px;
      background: var(--panel, #101721);
      box-shadow: 0 18px 48px rgba(0, 0, 0, 0.45);
      transform: translate(var(--window-x, 0px), var(--window-y, 0px));
    }
    :host([maximized]) .frame {
      width: 100%;
      height: 100%;
      max-width: 100%;
      border: 0;
      border-radius: 0;
      transform: none;
    }
    header {
      display: flex;
      align-items: center;
      gap: 8px;
      cursor: grab;
      user-select: none;
    }
    header:active { cursor: grabbing; }
    :host([maximized]) header { cursor: default; }
    .grip {
      flex: 0 0 auto;
      width: 8px;
      height: 14px;
      color: var(--muted, #8b9bad);
      background-image: radial-gradient(currentColor 1px, transparent 1.2px);
      background-size: 4px 4px;
      opacity: 0.7;
    }
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
    .control {
      display: grid;
      place-items: center;
      width: 30px;
      height: 26px;
      box-sizing: border-box;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 6px;
      background: rgba(125, 211, 252, 0.08);
      color: var(--text, #dbe6f2);
      padding: 0;
      font: 12px/1.4 var(--mono, monospace);
      cursor: pointer;
    }
    .icon {
      position: relative;
      display: block;
      width: 10px;
      height: 10px;
    }
    .icon-min {
      height: 0;
      margin-top: 4px;
      border-top: 1.5px solid currentColor;
    }
    .icon-max {
      border: 1.5px solid currentColor;
      border-radius: 1px;
    }
    .icon-restore {
      border: 1.5px solid currentColor;
      border-radius: 1px;
      transform: translate(1px, -1px);
    }
    .icon-restore::before {
      content: '';
      position: absolute;
      top: 2px;
      left: -4px;
      width: 8px;
      height: 8px;
      border: 1.5px solid currentColor;
      border-radius: 1px;
      background: var(--panel, #101721);
    }
    .icon-close::before,
    .icon-close::after {
      content: '';
      position: absolute;
      top: 4px;
      left: -1px;
      width: 12px;
      height: 1.5px;
      background: currentColor;
      transform: rotate(45deg);
    }
    .icon-close::after { transform: rotate(-45deg); }
    .control:hover { background: rgba(125, 211, 252, 0.16); }
    .control:focus-visible { outline: 2px solid var(--accent, #7dd3fc); outline-offset: 2px; }
    .body {
      display: flex;
      flex-direction: column;
      flex: 1 1 auto;
      min-width: 0;
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
      .control-max { display: none; }
    }
    @media (pointer: coarse) {
      .control { min-height: 44px; min-width: 44px; }
    }
  `
}

/**
 * The bar that holds minimized windows, shared by every `anth-window` on the
 * page. It lives on `document.body` so no panel can clip or scroll it.
 */
@customElement('anth-window-dock')
@localized()
export class AnthWindowDock extends LitElement {
  @state()
  private items: DockItem[] = []

  add(item: DockItem): void {
    this.items = [...this.items.filter((existing) => existing.id !== item.id), item]
  }

  drop(id: number): void {
    this.items = this.items.filter((item) => item.id !== id)
  }

  override render() {
    if (this.items.length === 0) {
      return nothing
    }
    return html`
      <div class="bar">
        ${this.items.map(
          (item) => html`
            <div class="chip">
              <button
                type="button"
                class="restore"
                title=${msg('Restore')}
                @click=${item.restore}
              >
                ${item.title}
              </button>
              <button
                type="button"
                class="dismiss"
                title=${msg('Close')}
                aria-label=${msg('Close')}
                @click=${item.close}
              >
                ×
              </button>
            </div>
          `,
        )}
      </div>
    `
  }

  static styles = css`
    :host {
      position: fixed;
      right: 0;
      bottom: 0;
      left: 0;
      z-index: 1000;
      display: block;
      pointer-events: none;
    }
    .bar {
      display: flex;
      gap: 6px;
      align-items: center;
      overflow-x: auto;
      padding: 6px max(12px, env(safe-area-inset-right))
        calc(6px + env(safe-area-inset-bottom)) max(12px, env(safe-area-inset-left));
      border-top: 1px solid var(--panel-border, #223041);
      background: rgba(16, 23, 33, 0.96);
      pointer-events: auto;
      backdrop-filter: blur(6px);
    }
    .chip {
      display: flex;
      flex: 0 0 auto;
      align-items: center;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 7px;
      background: rgba(125, 211, 252, 0.08);
      overflow: hidden;
    }
    .restore {
      max-width: 14rem;
      overflow: hidden;
      border: 0;
      background: transparent;
      color: var(--text, #dbe6f2);
      padding: 6px 9px;
      font: 12px/1.4 var(--mono, monospace);
      text-overflow: ellipsis;
      white-space: nowrap;
      cursor: pointer;
    }
    .restore:hover { background: rgba(125, 211, 252, 0.14); }
    .dismiss {
      border: 0;
      border-left: 1px solid var(--panel-border, #223041);
      background: transparent;
      color: var(--muted, #8b9bad);
      padding: 6px 9px;
      font: 12px/1.4 var(--mono, monospace);
      cursor: pointer;
    }
    .dismiss:hover { color: var(--err, #f87171); background: rgba(248, 113, 113, 0.12); }
    .restore:focus-visible,
    .dismiss:focus-visible {
      outline: 2px solid var(--accent, #7dd3fc);
      outline-offset: -2px;
    }
    @media (pointer: coarse) {
      .restore, .dismiss { min-height: 44px; }
    }
  `
}

let dock: AnthWindowDock | undefined
let windowIds = 0
let topZ = 100

function nextWindowId(): number {
  windowIds += 1
  return windowIds
}

function existingDock(): AnthWindowDock | undefined {
  return dock?.isConnected ? dock : undefined
}

function ensureDock(): AnthWindowDock {
  const connected = existingDock()
  if (connected) {
    return connected
  }
  dock = new AnthWindowDock()
  document.body.append(dock)
  return dock
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
