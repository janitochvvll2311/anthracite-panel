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
  ANTH_TERMINAL_READY_EVENT,
  ANTH_TERMINAL_RETRY_EVENT,
  ANTH_TERMINAL_STATUS_EVENT,
} from './terminal-view'
import { WebSocketTerminalTransport } from './lib/terminal/websocket-transport'
import type { TerminalStatus } from './lib/terminal/transport'

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
  private ctrl = false

  @state()
  private alt = false

  @state()
  private fontSize = 13

  @query('.screens')
  private screens!: HTMLElement

  private pinchDistance = 0
  private longPress?: ReturnType<typeof setTimeout>
  private gestureOrigin?: { x: number; y: number }
  private gestureActive = false
  private opening = false

  private newTransport(): WebSocketTerminalTransport {
    return new WebSocketTerminalTransport({
      url: (size) => `/api/terminal?cols=${size.cols}&rows=${size.rows}`,
    })
  }
  private tapStart?: { at: number; x: number; y: number }
  private lastTap?: { at: number; x: number; y: number }

  /** Number of open tabs. */
  get sessionCount(): number {
    return this.sessions.length
  }

  /** Focuses the active terminal after the window is shown. */
  focusActive(): void {
    this.sessions.find((session) => session.key === this.active)?.terminal.focus()
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
    wrapper.dataset.key = key

    const terminal = document.createElement('anth-terminal') as AnthTerminal
    terminal.chromeless = true
    terminal.autoFocus = true
    terminal.keyboardMode = 'auto'
    terminal.fontSize = this.fontSize
    const transport = this.newTransport()
    terminal.transport = transport

    terminal.addEventListener(ANTH_TERMINAL_READY_EVENT, (event) => {
      const detail = (event as CustomEvent<AnthTerminalReadyDetail>).detail
      this.notice = ''
      this.sessions = this.sessions.map((session) =>
        session.key === key
          ? { ...session, label: programLabel(detail.program), status: 'open' }
          : session,
      )
    })
    terminal.addEventListener(ANTH_TERMINAL_STATUS_EVENT, (event) => {
      const detail = (event as CustomEvent<AnthTerminalStatusDetail>).detail
      this.setStatus(key, detail.status)
    })
    terminal.addEventListener(ANTH_TERMINAL_EXIT_EVENT, (event) => {
      const detail = (event as CustomEvent<AnthTerminalExitDetail>).detail
      this.setStatus(key, 'closed')
      this.notice = msg(
        str`Shell exited with code ${detail.code}. Press Enter to reconnect.`,
      )
    })
    terminal.addEventListener(ANTH_TERMINAL_ERROR_EVENT, (event) => {
      const detail = (event as CustomEvent<AnthTerminalErrorDetail>).detail
      this.setStatus(key, 'error')
      this.notice = msg(str`${detail.message}. Press Enter to reconnect.`)
    })
    terminal.addEventListener(ANTH_TERMINAL_RETRY_EVENT, () => {
      void this.reconnect(key)
    })

    wrapper.append(terminal)
    this.screens.append(wrapper)
    const session: TerminalSession = {
      key,
      label: msg('Terminal'),
      status: 'connecting',
      terminal,
      transport,
      wrapper,
    }
    this.sessions = [...this.sessions, session]
    this.activate(key)
    this.opening = false
  }

  @state()
  private notice = ''

  private setStatus(key: string, status: TerminalStatus): void {
    this.sessions = this.sessions.map((session) =>
      session.key === key ? { ...session, status } : session,
    )
  }

  private activate(key: string): void {
    this.active = key
    for (const session of this.sessions) {
      session.wrapper.hidden = session.key !== key
    }
    const session = this.sessions.find((item) => item.key === key)
    session?.terminal.focus()
  }

  private async reconnect(key: string): Promise<void> {
    const session = this.sessions.find((item) => item.key === key)
    if (!session || session.status === 'connecting' || session.status === 'open') {
      return
    }
    const transport = this.newTransport()
    session.terminal.transport = transport
    this.sessions = this.sessions.map((item) =>
      item.key === key ? { ...item, transport, status: 'connecting' } : item,
    )
    this.notice = msg('Reconnecting…')
    session.terminal.focus()
  }

  private closeSession(key: string): void {
    const session = this.sessions.find((item) => item.key === key)
    if (!session) return
    session.transport.close()
    session.wrapper.remove()
    const remaining = this.sessions.filter((item) => item.key !== key)
    this.sessions = remaining
    if (remaining.length === 0) {
      this.dispatchEvent(
        new CustomEvent(TERMINAL_TABS_EMPTY_EVENT, { bubbles: true, composed: true }),
      )
      return
    }
    if (this.active === key) {
      this.activate(remaining[remaining.length - 1].key)
    }
  }

  private send(data: string): void {
    const session = this.sessions.find((item) => item.key === this.active)
    session?.terminal.send(data)
    this.ctrl = false
    this.alt = false
  }

  private sendHotkey(key: Hotkey): void {
    this.send(key.data)
  }

  private toggleModifier(which: 'ctrl' | 'alt'): void {
    if (which === 'ctrl') {
      this.ctrl = !this.ctrl
      if (this.ctrl) this.alt = false
    } else {
      this.alt = !this.alt
      if (this.alt) this.ctrl = false
    }
  }

  /** Applies a sticky modifier to the next character hotkey. */
  private sendCharacter(value: string): void {
    if (this.ctrl && value.length === 1) {
      const code = value.toUpperCase().charCodeAt(0)
      if (code >= 64 && code <= 95) {
        this.send(String.fromCharCode(code & 0x1f))
        return
      }
    }
    this.send(this.alt ? `\x1b${value}` : value)
  }

  private changeFont(delta: number): void {
    this.fontSize = Math.min(20, Math.max(10, this.fontSize + delta))
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

  private readonly onTouchStart = (event: TouchEvent): void => {
    if (event.touches.length === 2) {
      this.pinchDistance = touchDistance(event.touches)
      this.tapStart = undefined
      this.cancelLongPress()
      return
    }
    if (event.touches.length !== 1 || !this.active) return
    const touch = event.touches[0]
    this.gestureOrigin = { x: touch.clientX, y: touch.clientY }
    this.tapStart = { at: Date.now(), x: touch.clientX, y: touch.clientY }
    this.gestureActive = false
    this.longPress = setTimeout(() => {
      this.gestureActive = true
      this.tapStart = undefined
      this.gestureOrigin = { x: touch.clientX, y: touch.clientY }
      event.preventDefault()
    }, 450)
  }

  private readonly onTouchMove = (event: TouchEvent): void => {
    if (event.touches.length === 2) {
      const distance = touchDistance(event.touches)
      if (this.pinchDistance > 0 && Math.abs(distance - this.pinchDistance) > 18) {
        this.changeFont(distance > this.pinchDistance ? 1 : -1)
        this.pinchDistance = distance
      }
      event.preventDefault()
      return
    }
    if (!this.gestureActive || event.touches.length !== 1 || !this.gestureOrigin) {
      if (this.tapStart && event.touches[0]) {
        const touch = event.touches[0]
        if (
          Math.abs(touch.clientX - this.tapStart.x) > 12 ||
          Math.abs(touch.clientY - this.tapStart.y) > 12
        ) {
          this.tapStart = undefined
        }
      }
      if (this.longPress) {
        const touch = event.touches[0]
        if (
          touch &&
          (Math.abs(touch.clientX - (this.gestureOrigin?.x ?? touch.clientX)) > 12 ||
            Math.abs(touch.clientY - (this.gestureOrigin?.y ?? touch.clientY)) > 12)
        ) {
          this.cancelLongPress()
        }
      }
      return
    }
    const touch = event.touches[0]
    const dx = touch.clientX - this.gestureOrigin.x
    const dy = touch.clientY - this.gestureOrigin.y
    const threshold = 22
    if (Math.abs(dx) < threshold && Math.abs(dy) < threshold) return
    if (Math.abs(dx) > Math.abs(dy)) {
      this.send(dx > 0 ? '\x1b[C' : '\x1b[D')
    } else {
      this.send(dy > 0 ? '\x1b[B' : '\x1b[A')
    }
    this.gestureOrigin = { x: touch.clientX, y: touch.clientY }
    event.preventDefault()
  }

  private readonly onTouchEnd = (event: TouchEvent): void => {
    if (!this.gestureActive && this.tapStart && event.touches.length === 0) {
      const now = Date.now()
      const elapsed = now - this.tapStart.at
      if (elapsed < 300) {
        const last = this.lastTap
        if (
          last &&
          now - last.at < 350 &&
          Math.hypot(this.tapStart.x - last.x, this.tapStart.y - last.y) < 40
        ) {
          this.send('\t')
          event.preventDefault()
          this.lastTap = undefined
        } else {
          this.lastTap = { at: now, x: this.tapStart.x, y: this.tapStart.y }
        }
      }
    }
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

  private statusLabel(status: TerminalStatus): string {
    switch (status) {
      case 'idle':
        return msg('Idle')
      case 'connecting':
        return msg('Connecting')
      case 'open':
        return msg('Connected')
      case 'closed':
        return msg('Closed')
      case 'error':
        return msg('Error')
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
                    @click=${() => this.activate(session.key)}
                  >
                    <span class="dot" data-status=${session.status}></span>
                    <span class="tab-label">${session.label}</span>
                    <span class="tab-status">${this.statusLabel(session.status)}</span>
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
            <button
              type="button"
              class="hotkey modifier"
              aria-pressed=${this.ctrl}
              @click=${() => this.toggleModifier('ctrl')}
            >
              Ctrl
            </button>
            <button
              type="button"
              class="hotkey modifier"
              aria-pressed=${this.alt}
              @click=${() => this.toggleModifier('alt')}
            >
              Alt
            </button>
            ${HOTKEYS.slice(0, 2).map(
              (key) => html`<button type="button" class="hotkey" @click=${() => this.sendHotkey(key)}>${key.label}</button>`,
            )}
            ${['Up', 'Dn', 'Rt', 'Lt'].map(
              (label) => {
                const key = HOTKEYS.find((item) => item.label === label)!
                return html`<button type="button" class="hotkey" @click=${() => this.sendHotkey(key)}>${label}</button>`
              },
            )}
            ${HOTKEYS.slice(6).map(
              (key) =>
                html`<button
                  type="button"
                  class="hotkey"
                  @click=${() =>
                    key.data.length === 1 ? this.sendCharacter(key.data) : this.sendHotkey(key)}
                >
                  ${key.label}
                </button>`,
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

function touchDistance(touches: TouchList): number {
  const [left, right] = [touches[0], touches[1]]
  return Math.hypot(right.clientX - left.clientX, right.clientY - left.clientY)
}
