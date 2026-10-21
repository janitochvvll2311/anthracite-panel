import { LitElement, css, html, nothing } from 'lit'
import type { PropertyValues } from 'lit'
import { customElement, property, query, state } from 'lit/decorators.js'
import { msg } from '@lit/localize'

import {
  TITLE_BAR_HEIGHT,
  dragRect,
  resizeRect,
  type Rect,
  type Viewport,
} from '../lib/ui/windows'

/**
 * One floating window: a title bar, a body and a resize handle.
 *
 * The component is controlled. It renders the `rect` and `minimized` it is
 * given and reports what the operator did, so the shell owns the layout and can
 * persist it and keep the z order. That also means a drag is a stream of events
 * rather than of state changes: `anth-window-move` fires while the pointer
 * moves, and the shell decides whether to write it to storage.
 *
 * The body is a slot, so the panel inside is a normal element. A window that is
 * minimized keeps the panel mounted but out of the flow, and the shell decides
 * whether to detach it instead; this component never unmounts its content,
 * because losing a terminal session to a minimize would be a bug.
 */

export const ANTH_WINDOW_MOVE_EVENT = 'anth-window-move'
export const ANTH_WINDOW_RESIZE_EVENT = 'anth-window-resize'
export const ANTH_WINDOW_CLOSE_EVENT = 'anth-window-close'
export const ANTH_WINDOW_MINIMIZE_EVENT = 'anth-window-minimize'
export const ANTH_WINDOW_FOCUS_EVENT = 'anth-window-focus'
export const ANTH_WINDOW_POPOUT_EVENT = 'anth-window-popout'

export interface WindowMoveDetail {
  id: string
  rect: Rect
}

export interface WindowIdDetail {
  id: string
}

@customElement('anth-window')
export class AnthWindow extends LitElement {
  @property({ attribute: 'window-id' })
  windowId = ''

  @property()
  heading = ''

  @property({ attribute: false })
  rect: Rect = { x: 40, y: 40, width: 640, height: 420 }

  @property({ type: Boolean, reflect: true })
  minimized = false

  @property({ type: Number })
  z = 1

  @property({ type: Boolean })
  active = false

  /** Shows the pop-out button. A window without a URL cannot leave the page. */
  @property({ type: Boolean, attribute: 'popout-url' })
  canPopout = false

  @query('.frame')
  private frame?: HTMLElement

  /** The rect at the moment a drag started, for delta maths. */
  private dragOrigin?: { rect: Rect; x: number; y: number }

  @state()
  private dragging = false

  @state()
  private resizing = false

  static override styles = css`
    :host {
      position: absolute;
      display: block;
      pointer-events: none;
    }

    .frame {
      display: flex;
      flex-direction: column;
      overflow: hidden;
      border: 1px solid light-dark(#c3cad4, #3b424c);
      border-radius: 10px;
      background: light-dark(#ffffff, #1b1f24);
      box-shadow: 0 12px 32px rgb(0 0 0 / 22%);
      pointer-events: auto;
      block-size: 100%;
    }

    :host([active]) .frame {
      border-color: light-dark(#6b7fd7, #8e9ff0);
      box-shadow: 0 16px 40px rgb(0 0 0 / 30%);
    }

    :host([minimized]) {
      display: none;
    }

    .title {
      display: flex;
      align-items: center;
      gap: 0.5rem;
      padding-inline: 0.65rem;
      block-size: ${TITLE_BAR_HEIGHT}px;
      box-sizing: border-box;
      border-block-end: 1px solid light-dark(#e2e6ec, #2c3238);
      background: light-dark(#f4f6f9, #23282e);
      cursor: grab;
      user-select: none;
      touch-action: none;
    }

    .frame[data-dragging] .title {
      cursor: grabbing;
    }

    .heading {
      flex: 1;
      overflow: hidden;
      font-size: 0.85rem;
      font-weight: 600;
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    button {
      display: inline-flex;
      align-items: center;
      justify-content: center;
      inline-size: 1.6rem;
      block-size: 1.6rem;
      border: none;
      border-radius: 4px;
      background: none;
      color: inherit;
      cursor: pointer;
      font: inherit;
      line-height: 1;
    }

    button:hover {
      background: light-dark(#e2e6ec, #2f363d);
    }

    button:focus-visible {
      outline: 2px solid light-dark(#6b7fd7, #8e9ff0);
      outline-offset: 1px;
    }

    button[data-danger]:hover {
      background: light-dark(#f6d7d7, #5a2a2a);
    }

    .body {
      flex: 1;
      min-block-size: 0;
      overflow: auto;
    }

    .resize {
      position: absolute;
      inset-block-end: 0;
      inset-inline-end: 0;
      inline-size: 16px;
      block-size: 16px;
      cursor: nwse-resize;
      touch-action: none;
      background: linear-gradient(
        135deg,
        transparent 50%,
        light-dark(#b9c0ca, #4a515a) 50%
      );
      border-end-end-radius: 10px;
    }
  `

  override connectedCallback(): void {
    super.connectedCallback()
    this.setAttribute('role', 'dialog')
    this.setAttribute('aria-label', this.heading)
  }

  protected override updated(changed: PropertyValues<this>): void {
    if (changed.has('heading')) this.setAttribute('aria-label', this.heading)
  }

  override render() {
    const style = [
      `inset-inline-start:${this.rect.x}px`,
      `inset-block-start:${this.rect.y}px`,
      `inline-size:${this.rect.width}px`,
      `block-size:${this.rect.height}px`,
      `z-index:${this.z}`,
    ].join(';')

    return html`
      <div
        class="frame"
        style=${style}
        ?data-dragging=${this.dragging || this.resizing}
        @pointerdown=${this.onFocus}
        @pointermove=${this.onPointerMove}
        @pointerup=${this.onPointerUp}
        @pointercancel=${this.onPointerUp}
      >
        <header
          class="title"
          @pointerdown=${this.onDragStart}
          @dblclick=${this.onMinimize}
        >
          <span class="heading">${this.heading}</span>
          ${this.canPopout
            ? html`<button
                type="button"
                title=${msg('Open in a separate window')}
                aria-label=${msg('Open in a separate window')}
                @pointerdown=${this.stop}
                @click=${this.onPopout}
              >
                ⧉
              </button>`
            : nothing}
          <button
            type="button"
            title=${msg('Minimize')}
            aria-label=${msg('Minimize')}
            @pointerdown=${this.stop}
            @click=${this.onMinimize}
          >
            –
          </button>
          <button
            type="button"
            title=${msg('Close')}
            aria-label=${msg('Close')}
            data-danger
            @pointerdown=${this.stop}
            @click=${this.onClose}
          >
            ×
          </button>
        </header>
        <div class="body"><slot></slot></div>
        <div
          class="resize"
          role="separator"
          aria-label=${msg('Resize')}
          @pointerdown=${this.onResizeStart}
        ></div>
      </div>
    `
  }

  private stop(event: Event): void {
    // A press on a button must not start a drag of the window underneath it.
    event.stopPropagation()
  }

  private readonly onFocus = (): void => {
    this.dispatchEvent(
      new CustomEvent<WindowIdDetail>(ANTH_WINDOW_FOCUS_EVENT, {
        detail: { id: this.windowId },
        bubbles: true,
        composed: true,
      }),
    )
  }

  private readonly onMinimize = (event: Event): void => {
    event.stopPropagation()
    this.dispatchEvent(
      new CustomEvent<WindowIdDetail>(ANTH_WINDOW_MINIMIZE_EVENT, {
        detail: { id: this.windowId },
        bubbles: true,
        composed: true,
      }),
    )
  }

  private readonly onClose = (): void => {
    this.dispatchEvent(
      new CustomEvent<WindowIdDetail>(ANTH_WINDOW_CLOSE_EVENT, {
        detail: { id: this.windowId },
        bubbles: true,
        composed: true,
      }),
    )
  }

  private readonly onPopout = (): void => {
    this.dispatchEvent(
      new CustomEvent<WindowIdDetail>(ANTH_WINDOW_POPOUT_EVENT, {
        detail: { id: this.windowId },
        bubbles: true,
        composed: true,
      }),
    )
  }

  private readonly onDragStart = (event: PointerEvent): void => {
    if (event.button !== 0) return
    // A press on a button inside the title bar is not a drag.
    if ((event.target as HTMLElement).closest('button')) return
    event.preventDefault()
    this.dragOrigin = { rect: { ...this.rect }, x: event.clientX, y: event.clientY }
    this.dragging = true
    this.frame?.setPointerCapture(event.pointerId)
    this.onFocus()
  }

  private readonly onResizeStart = (event: PointerEvent): void => {
    if (event.button !== 0) return
    event.preventDefault()
    event.stopPropagation()
    this.dragOrigin = { rect: { ...this.rect }, x: event.clientX, y: event.clientY }
    this.resizing = true
    this.frame?.setPointerCapture(event.pointerId)
    this.onFocus()
  }

  private readonly onPointerMove = (event: PointerEvent): void => {
    const origin = this.dragOrigin
    if (!origin) return
    const delta = { dx: event.clientX - origin.x, dy: event.clientY - origin.y }
    const rect = this.resizing
      ? resizeRect(origin.rect, delta, this.viewport())
      : dragRect(origin.rect, delta, this.viewport())
    this.emit(
      this.resizing ? ANTH_WINDOW_RESIZE_EVENT : ANTH_WINDOW_MOVE_EVENT,
      rect,
    )
  }

  private readonly onPointerUp = (event: PointerEvent): void => {
    if (!this.dragOrigin) return
    this.dragOrigin = undefined
    this.dragging = false
    this.resizing = false
    this.frame?.releasePointerCapture(event.pointerId)
  }

  private viewport(): Viewport {
    return {
      width: globalThis.innerWidth ?? 1024,
      height: globalThis.innerHeight ?? 768,
    }
  }

  private emit(type: string, rect: Rect): void {
    this.dispatchEvent(
      new CustomEvent<WindowMoveDetail>(type, {
        detail: { id: this.windowId, rect },
        bubbles: true,
        composed: true,
      }),
    )
  }
}
