import { LitElement, css, html, nothing } from 'lit'
import { customElement, query, state } from 'lit/decorators.js'
import { localized, msg, str } from '@lit/localize'

import './icon'
import './terminal-view'
import type {
  AnthTerminal,
  AnthTerminalErrorDetail,
  AnthTerminalExitDetail,
  AnthTerminalReadyDetail,
  AnthTerminalStatusDetail,
} from './terminal-view'
import {
  ANTH_TERMINAL_ERROR_EVENT,
  ANTH_TERMINAL_EXIT_EVENT,
  ANTH_TERMINAL_MODIFIER_EVENT,
  ANTH_TERMINAL_READY_EVENT,
  ANTH_TERMINAL_RETRY_EVENT,
  ANTH_TERMINAL_STATUS_EVENT,
} from './terminal-view'
import { applyStickyModifiers } from './lib/terminal/modifiers'
import { WebSocketTerminalTransport } from 'anth-wire'
import type { TerminalStatus } from 'anth-wire'

/** Fired when the last terminal tab closes. */
export const TERMINAL_TABS_EMPTY_EVENT = 'anth-terminal-tabs-empty'

interface TerminalSession {
  key: string
  label: string
  status: TerminalStatus
  terminal: AnthTerminal
  transport: WebSocketTerminalTransport
  wrapper: HTMLElement
}

interface Hotkey {
  label: string
  data: string
}

interface Point {
  x: number
  y: number
}

/** Special keys, in the order the bar shows them. */
const HOTKEYS: Hotkey[] = [
  { label: 'Esc', data: '\x1b' },
  { label: 'Tab', data: '\t' },
  { label: 'Up', data: '\x1b[A' },
  { label: 'Dn', data: '\x1b[B' },
  { label: 'Rt', data: '\x1b[C' },
  { label: 'Lt', data: '\x1b[D' },
  { label: 'Home', data: '\x1b[H' },
  { label: 'End', data: '\x1b[F' },
  { label: 'PgUp', data: '\x1b[5~' },
  { label: 'PgDn', data: '\x1b[6~' },
  { label: '^C', data: '\x03' },
  { label: '^D', data: '\x04' },
  { label: '^L', data: '\x0c' },
  { label: '^Z', data: '\x1a' },
  { label: '|', data: '|' },
  { label: '~', data: '~' },
  { label: '/', data: '/' },
  { label: '-', data: '-' },
  { label: '$', data: '$' },
  { label: '*', data: '*' },
]

const FONT_SIZE = { min: 10, max: 20, step: 1 }

/** Touch thresholds in pixels and milliseconds. */
const GESTURE = {
  longPressMs: 450,
  tapMs: 300,
  doubleTapMs: 350,
  tapSlop: 12,
  doubleTapSlop: 40,
  dragStep: 22,
  pinchStep: 18,
  hotkeySlop: 10,
}

const STATUS_LABELS: Record<TerminalStatus, () => string> = {
  idle: () => msg('Idle'),
  connecting: () => msg('Connecting'),
  open: () => msg('Connected'),
  closed: () => msg('Closed'),
  error: () => msg('Error'),
}

let nextKey = 0

/**
 * The tab strip, terminal surfaces and mobile keyboard of the global terminal
 * window. One tab is one WebSocket and one PTY.
 */
@customElement('anth-terminal-tabs')
@localized()
export class AnthTerminalTabs extends LitElement {
  @state()
  private sessions: TerminalSession[] = []

  @state()
  private active = ''

  @state()
  private notice = ''

  @state()
  private ctrl = false

  @state()
  private alt = false

  @state()
  private fontSize = 13

  @query('.screens')
  private screens!: HTMLElement

  private opening = false
  private pinchDistance = 0
  private longPress?: ReturnType<typeof setTimeout>
  private gestureOrigin?: Point
  private gestureActive = false
  private tapStart?: Point & { at: number }
  private lastTap?: Point & { at: number }
  private touchHotkey?: Point & { action: () => void }

  /** Number of open tabs. */
  get sessionCount(): number {
    return this.sessions.length
  }

  /** Focuses the active terminal after the window is shown. */
  focusActive(): void {
    this.activeSession?.terminal.focus()
  }

  /** Opens a new session and makes it active. */
  async openSession(): Promise<void> {
    if (this.opening) return
    this.opening = true
    await this.updateComplete
    if (!this.screens) {
      this.opening = false
      return
    }

    const key = `terminal-${++nextKey}`
    const wrapper = document.createElement('div')
    wrapper.className = 'screen-slot'

    const terminal = document.createElement('anth-terminal') as AnthTerminal
    terminal.chromeless = true
    terminal.autoFocus = true
    terminal.keyboardMode = 'auto'
    terminal.fontSize = this.fontSize
    const transport = this.newTransport()
    terminal.transport = transport

    terminal.addEventListener(ANTH_TERMINAL_READY_EVENT, (event) => {
      const { program } = (event as CustomEvent<AnthTerminalReadyDetail>).detail
      this.notice = ''
      this.updateSession(key, { label: programLabel(program), status: 'open' })
    })
    terminal.addEventListener(ANTH_TERMINAL_STATUS_EVENT, (event) => {
      const { status } = (event as CustomEvent<AnthTerminalStatusDetail>).detail
      this.updateSession(key, { status })
    })
    terminal.addEventListener(ANTH_TERMINAL_EXIT_EVENT, (event) => {
      const { code } = (event as CustomEvent<AnthTerminalExitDetail>).detail
      this.updateSession(key, { status: 'closed' })
      this.notice = msg(
        str`Shell exited with code ${code}. Press Enter to reconnect.`,
      )
    })
    terminal.addEventListener(ANTH_TERMINAL_ERROR_EVENT, (event) => {
      const { message } = (event as CustomEvent<AnthTerminalErrorDetail>).detail
      this.updateSession(key, { status: 'error' })
      this.notice = msg(str`${message}. Press Enter to reconnect.`)
    })
    terminal.addEventListener(ANTH_TERMINAL_RETRY_EVENT, () => {
      void this.reconnect(key)
    })
    terminal.addEventListener(ANTH_TERMINAL_MODIFIER_EVENT, () => {
      this.ctrl = false
      this.alt = false
    })

    wrapper.append(terminal)
    this.screens.append(wrapper)
    this.sessions = [
      ...this.sessions,
      {
        key,
        label: msg('Terminal'),
        status: 'connecting',
        terminal,
        transport,
        wrapper,
      },
    ]
    this.select(key)
    this.opening = false
  }

  /** Makes `key` the visible tab and focuses its terminal. */
  private select(key: string): void {
    this.active = key
    for (const session of this.sessions) {
      session.wrapper.hidden = session.key !== key
    }
    this.activeSession?.terminal.focus()
    this.pushModifiers()
  }

  private async reconnect(key: string): Promise<void> {
    const session = this.sessionByKey(key)
    if (!session || session.status === 'connecting' || session.status === 'open') {
      return
    }
    const transport = this.newTransport()
    session.terminal.transport = transport
    this.updateSession(key, { transport, status: 'connecting' })
    this.notice = msg('Reconnecting…')
    session.terminal.focus()
    this.pushModifiers()
  }

  private closeSession(key: string): void {
    const session = this.sessionByKey(key)
    if (!session) return
    session.transport.close()
    session.wrapper.remove()
    const remaining = this.sessions.filter((item) => item.key !== key)
    this.sessions = remaining
    if (remaining.length === 0) {
      this.dispatchEvent(
        new CustomEvent(TERMINAL_TABS_EMPTY_EVENT, {
          bubbles: true,
          composed: true,
        }),
      )
      return
    }
    if (this.active === key) {
      this.select(remaining[remaining.length - 1].key)
    }
  }

  private newTransport(): WebSocketTerminalTransport {
    return new WebSocketTerminalTransport({
      url: (size) => `/api/terminal?cols=${size.cols}&rows=${size.rows}`,
    })
  }

  private get activeSession(): TerminalSession | undefined {
    return this.sessionByKey(this.active)
  }

  private sessionByKey(key: string): TerminalSession | undefined {
    return this.sessions.find((session) => session.key === key)
  }

  private updateSession(key: string, patch: Partial<TerminalSession>): void {
    this.sessions = this.sessions.map((session) =>
      session.key === key ? { ...session, ...patch } : session,
    )
  }

  private send(data: string): void {
    this.activeSession?.terminal.send(data)
    this.ctrl = false
    this.alt = false
    this.pushModifiers()
  }

  /**
   * Sends one key from the bar. Character keys honour the sticky modifiers;
   * escape sequences go through unchanged.
   */
  private sendKey(key: Hotkey): void {
    this.send(
      key.data.length === 1
        ? applyStickyModifiers(key.data, this.ctrl, this.alt)
        : key.data,
    )
  }

  /** Mirrors the sticky buttons into the active terminal. */
  private pushModifiers(): void {
    const session = this.activeSession
    if (!session) return
    session.terminal.stickyCtrl = this.ctrl
    session.terminal.stickyAlt = this.alt
  }

  /** Pointer-based activation: mouse and pen go down, touch waits for the up. */
  private readonly pressHotkey = (event: PointerEvent, action: () => void): void => {
    if (event.pointerType === 'touch' || event.button > 0) return
    event.preventDefault()
    this.activateHotkey(action)
  }

  /**
   * Remembers where the finger went down so a tap can be told apart from a
   * horizontal scroll of the key bar.
   */
  private readonly startHotkey = (event: TouchEvent, action: () => void): void => {
    if (event.touches.length !== 1) {
      this.touchHotkey = undefined
      return
    }
    this.touchHotkey = {
      x: event.touches[0].clientX,
      y: event.touches[0].clientY,
      action,
    }
  }

  /**
   * Sends the key on `touchend` when the finger did not move.
   *
   * Android drops the `click` of a button once the soft keyboard closes and the
   * window resizes, and `touchstart` cannot be cancelled without breaking the
   * bar's horizontal scroll. Cancelling `touchend` stops the button from taking
   * focus (which closes the keyboard) while a real drag is left untouched.
   */
  private readonly endHotkey = (event: TouchEvent): void => {
    const pending = this.touchHotkey
    this.touchHotkey = undefined
    if (!pending) return
    const touch = event.changedTouches[0]
    if (touch && moved(pointOf(touch), pending, GESTURE.hotkeySlop)) return
    event.preventDefault()
    this.activateHotkey(pending.action)
  }

  /** Keyboard activation (Enter/Space) arrives as a detail-0 click. */
  private readonly clickHotkey = (event: MouseEvent, action: () => void): void => {
    if (event.detail !== 0) return
    this.activateHotkey(action)
  }

  private activateHotkey(action: () => void): void {
    this.activeSession?.terminal.focus()
    action()
  }

  private renderHotkey(label: string, action: () => void, pressed?: boolean) {
    return html`
      <button
        type="button"
        class=${pressed === undefined ? 'hotkey' : 'hotkey modifier'}
        aria-pressed=${pressed === undefined ? nothing : pressed}
        @mousedown=${(event: MouseEvent) => event.preventDefault()}
        @pointerdown=${(event: PointerEvent) => this.pressHotkey(event, action)}
        @touchstart=${(event: TouchEvent) => this.startHotkey(event, action)}
        @touchend=${this.endHotkey}
        @touchcancel=${() => {
          this.touchHotkey = undefined
        }}
        @click=${(event: MouseEvent) => this.clickHotkey(event, action)}
      >
        ${label}
      </button>
    `
  }

  private toggleModifier(which: 'ctrl' | 'alt'): void {
    if (which === 'ctrl') {
      this.ctrl = !this.ctrl
      if (this.ctrl) this.alt = false
    } else {
      this.alt = !this.alt
      if (this.alt) this.ctrl = false
    }
    this.pushModifiers()
  }

  private changeFont(delta: number): void {
    this.fontSize = Math.min(
      FONT_SIZE.max,
      Math.max(FONT_SIZE.min, this.fontSize + delta * FONT_SIZE.step),
    )
    for (const session of this.sessions) {
      session.terminal.fontSize = this.fontSize
      session.terminal.fit()
    }
  }

  private hideKeyboard(): void {
    for (const session of this.sessions) {
      session.terminal.blur()
    }
  }

  override connectedCallback(): void {
    super.connectedCallback()
    this.addEventListener('touchstart', this.onTouchStart, { passive: false })
    this.addEventListener('touchmove', this.onTouchMove, { passive: false })
    this.addEventListener('touchend', this.onTouchEnd, { passive: false })
    this.addEventListener('touchcancel', this.onTouchEnd)
  }

  override disconnectedCallback(): void {
    super.disconnectedCallback()
    this.removeEventListener('touchstart', this.onTouchStart)
    this.removeEventListener('touchmove', this.onTouchMove)
    this.removeEventListener('touchend', this.onTouchEnd)
    this.removeEventListener('touchcancel', this.onTouchEnd)
    for (const session of this.sessions) {
      session.transport.close()
    }
  }

  /**
   * Taps on tabs, buttons or the hotkey bar must not drive terminal gestures.
   *
   * The listener sits on the host, so `event.target` is retargeted to
   * `anth-terminal-tabs`; the real element comes from `composedPath()`.
   */
  private static touchesControl(event: Event): boolean {
    const target = event.composedPath()[0]
    return (
      target instanceof Element &&
      target.closest('button, .tabs, .hotkeys') !== null
    )
  }

  private readonly onTouchStart = (event: TouchEvent): void => {
    if (AnthTerminalTabs.touchesControl(event)) {
      this.resetGesture()
      return
    }
    if (event.touches.length === 2) {
      this.resetGesture()
      this.pinchDistance = touchDistance(event.touches)
      return
    }
    if (event.touches.length !== 1 || !this.active) return
    const touch = event.touches[0]
    this.gestureOrigin = pointOf(touch)
    this.tapStart = { ...pointOf(touch), at: Date.now() }
    this.gestureActive = false
    this.longPress = setTimeout(() => {
      this.gestureActive = true
      this.tapStart = undefined
      this.gestureOrigin = pointOf(touch)
    }, GESTURE.longPressMs)
  }

  private readonly onTouchMove = (event: TouchEvent): void => {
    if (event.touches.length === 2) {
      const distance = touchDistance(event.touches)
      if (
        this.pinchDistance > 0 &&
        Math.abs(distance - this.pinchDistance) > GESTURE.pinchStep
      ) {
        this.changeFont(distance > this.pinchDistance ? 1 : -1)
        this.pinchDistance = distance
      }
      event.preventDefault()
      return
    }

    const touch = event.touches[0]
    if (!this.gestureActive || !touch || !this.gestureOrigin) {
      // A moving finger is a scroll or a selection, not a tap.
      if (touch && this.tapStart && moved(pointOf(touch), this.tapStart, GESTURE.tapSlop)) {
        this.tapStart = undefined
      }
      if (touch && moved(pointOf(touch), this.gestureOrigin ?? pointOf(touch), GESTURE.tapSlop)) {
        this.cancelLongPress()
      }
      return
    }

    const dx = touch.clientX - this.gestureOrigin.x
    const dy = touch.clientY - this.gestureOrigin.y
    if (Math.abs(dx) < GESTURE.dragStep && Math.abs(dy) < GESTURE.dragStep) {
      return
    }
    if (Math.abs(dx) > Math.abs(dy)) {
      this.send(dx > 0 ? '\x1b[C' : '\x1b[D')
    } else {
      this.send(dy > 0 ? '\x1b[B' : '\x1b[A')
    }
    this.gestureOrigin = pointOf(touch)
    event.preventDefault()
  }

  private readonly onTouchEnd = (event: TouchEvent): void => {
    if (AnthTerminalTabs.touchesControl(event)) return
    if (!this.gestureActive && this.tapStart && event.touches.length === 0) {
      const now = Date.now()
      if (now - this.tapStart.at < GESTURE.tapMs) {
        const last = this.lastTap
        if (
          last &&
          now - last.at < GESTURE.doubleTapMs &&
          !moved(this.tapStart, last, GESTURE.doubleTapSlop)
        ) {
          this.send('\t')
          event.preventDefault()
          this.lastTap = undefined
        } else {
          this.lastTap = { ...this.tapStart }
        }
      }
    }
    this.resetGesture()
  }

  private resetGesture(): void {
    this.tapStart = undefined
    this.cancelLongPress()
    this.gestureOrigin = undefined
    this.gestureActive = false
    this.pinchDistance = 0
  }

  private cancelLongPress(): void {
    if (this.longPress) {
      clearTimeout(this.longPress)
      this.longPress = undefined
    }
  }

  override render() {
    return html`
      <div class="terminal-shell">
        <header class="tabs">
          <div class="tab-list" role="tablist" aria-label=${msg('Terminal sessions')}>
            ${this.sessions.map(
              (session) => html`
                <div class="tab" data-active=${session.key === this.active ? '' : nothing}>
                  <button
                    type="button"
                    class="tab-main"
                    role="tab"
                    aria-selected=${session.key === this.active}
                    @click=${() => this.select(session.key)}
                  >
                    <span class="dot" data-status=${session.status}></span>
                    <span class="tab-label">${session.label}</span>
                    <span class="tab-status">${STATUS_LABELS[session.status]()}</span>
                  </button>
                  <button
                    type="button"
                    class="tab-close"
                    aria-label=${msg('Close session')}
                    @click=${() => this.closeSession(session.key)}
                  >
                    <anth-icon name="close"></anth-icon>
                  </button>
                </div>
              `,
            )}
            <button
              type="button"
              class="icon-button new-tab"
              aria-label=${msg('New terminal')}
              title=${msg('New terminal')}
              @click=${() => void this.openSession()}
            >
              <anth-icon name="plus"></anth-icon>
            </button>
          </div>
          <div class="tab-actions">
            <button
              type="button"
              class="icon-button"
              aria-label=${msg('Smaller text')}
              title=${msg('Smaller text')}
              @click=${() => this.changeFont(-1)}
            >
              <span aria-hidden="true">A−</span>
            </button>
            <button
              type="button"
              class="icon-button"
              aria-label=${msg('Larger text')}
              title=${msg('Larger text')}
              @click=${() => this.changeFont(1)}
            >
              <span aria-hidden="true">A+</span>
            </button>
            <button
              type="button"
              class="icon-button"
              aria-label=${msg('Hide keyboard')}
              title=${msg('Hide keyboard')}
              @click=${() => this.hideKeyboard()}
            >
              <anth-icon name="chevron-down"></anth-icon>
            </button>
          </div>
        </header>

        ${this.notice
          ? html`<div class="notice" role="status">${this.notice}</div>`
          : nothing}

        <div class="screens"></div>

        <div class="hotkeys" role="toolbar" aria-label=${msg('Terminal keys')}>
          <div class="hotkey-row">
            ${this.renderHotkey('Ctrl', () => this.toggleModifier('ctrl'), this.ctrl)}
            ${this.renderHotkey('Alt', () => this.toggleModifier('alt'), this.alt)}
            ${HOTKEYS.map((key) =>
              this.renderHotkey(key.label, () => this.sendKey(key)),
            )}
          </div>
        </div>
      </div>
    `
  }

  static styles = css`
    :host {
      display: block;
      height: 100%;
      min-height: 0;
      color: var(--text, #dbe6f2);
      font: 13px/1.4 var(--mono, monospace);
    }
    .terminal-shell {
      display: flex;
      flex-direction: column;
      height: 100%;
      min-height: 0;
      background: var(--terminal-bg, #0b0f14);
    }
    .tabs {
      display: flex;
      flex: 0 0 auto;
      align-items: center;
      gap: 6px;
      padding: 6px;
      border-bottom: 1px solid var(--panel-border, #223041);
      background: var(--terminal-surface, #131a23);
    }
    .tab-list {
      display: flex;
      flex: 1 1 auto;
      gap: 4px;
      min-width: 0;
      overflow-x: auto;
      scrollbar-width: thin;
    }
    .tab {
      display: flex;
      flex: 0 0 auto;
      align-items: center;
      min-width: 0;
      border: 1px solid transparent;
      border-radius: 6px;
      background: transparent;
    }
    .tab[data-active] {
      border-color: var(--panel-border, #223041);
      background: rgba(125, 211, 252, 0.08);
    }
    .tab-main,
    .tab-close,
    .icon-button,
    .hotkey {
      border: 1px solid transparent;
      background: transparent;
      color: var(--text, #dbe6f2);
      cursor: pointer;
      font: inherit;
    }
    .tab-main {
      display: inline-flex;
      align-items: center;
      gap: 6px;
      min-width: 0;
      max-width: 15rem;
      padding: 6px 4px 6px 8px;
      text-align: left;
    }
    .tab-label {
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }
    .tab-status {
      color: var(--muted, #8b9bad);
      font-size: 10px;
      white-space: nowrap;
    }
    .dot {
      width: 6px;
      height: 6px;
      border-radius: 50%;
      background: var(--muted, #8b9bad);
    }
    .dot[data-status='open'] { background: #34d399; }
    .dot[data-status='connecting'] { background: #fbbf24; }
    .dot[data-status='error'] { background: #f87171; }
    .tab-close,
    .icon-button {
      display: inline-grid;
      place-items: center;
      width: 28px;
      height: 28px;
      padding: 0;
      border-radius: 5px;
    }
    .tab-close:hover,
    .icon-button:hover { border-color: var(--panel-border, #223041); }
    .new-tab { flex: 0 0 auto; }
    .tab-actions { display: flex; flex: 0 0 auto; gap: 2px; }
    .screens {
      position: relative;
      flex: 1 1 auto;
      min-height: 0;
      overflow: hidden;
    }
    .screen-slot {
      position: absolute;
      inset: 0;
      display: flex;
      min-height: 0;
    }
    .screen-slot > anth-terminal {
      flex: 1 1 auto;
      width: 100%;
      height: 100%;
    }
    .screen-slot[hidden] { display: none; }
    .notice {
      flex: 0 0 auto;
      padding: 4px 8px;
      color: #a7f3d0;
      background: rgba(52, 211, 153, 0.12);
      font-size: 11px;
    }
    .hotkeys {
      flex: 0 0 auto;
      padding: 5px;
      border-top: 1px solid var(--panel-border, #223041);
      background: var(--terminal-surface, #131a23);
      overflow-x: auto;
    }
    .hotkey-row {
      display: flex;
      gap: 4px;
      min-width: max-content;
    }
    .hotkey {
      min-width: 36px;
      min-height: 34px;
      padding: 0 7px;
      border-color: var(--panel-border, #223041);
      border-radius: 5px;
      background: rgba(125, 211, 252, 0.06);
      font-family: var(--sans, system-ui, sans-serif);
      touch-action: manipulation;
    }
    .hotkey:active { background: rgba(125, 211, 252, 0.18); }
    .hotkey[aria-pressed='true'] {
      border-color: var(--accent, #7dd3fc);
      background: rgba(125, 211, 252, 0.2);
      color: var(--accent, #7dd3fc);
    }
    @media (max-width: 48rem) {
      .tab-status { display: none; }
      .tab-main { max-width: 9rem; }
    }
  `
}

function programLabel(program: string): string {
  const name = program.split('/').filter(Boolean).pop()
  return name && name !== '' ? name : program
}

function pointOf(touch: Touch): Point {
  return { x: touch.clientX, y: touch.clientY }
}

/** Whether `touch` is farther than `slop` pixels from `origin`. */
function moved(touch: Point, origin: Point, slop: number): boolean {
  return (
    Math.abs(touch.x - origin.x) > slop || Math.abs(touch.y - origin.y) > slop
  )
}

function touchDistance(touches: TouchList): number {
  const [left, right] = [touches[0], touches[1]]
  return Math.hypot(right.clientX - left.clientX, right.clientY - left.clientY)
}
