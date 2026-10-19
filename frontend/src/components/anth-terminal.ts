import { LitElement, css, html, nothing, unsafeCSS } from 'lit'
import type { PropertyValues } from 'lit'
import { customElement, property, query, state } from 'lit/decorators.js'
import xtermCss from '@xterm/xterm/css/xterm.css?inline'
import type { ITerminalOptions, ITheme, Terminal } from '@xterm/xterm'
import type { FitAddon } from '@xterm/addon-fit'
import type {
  TerminalSessionInfo,
  TerminalSize,
  TerminalStatus,
  TerminalTransport,
} from '../lib/terminal/transport'

/** Event fired once the transport has reported a ready shell. */
export const ANTH_TERMINAL_READY_EVENT = 'anth-terminal-ready'
/** Event fired when the remote process exits. */
export const ANTH_TERMINAL_EXIT_EVENT = 'anth-terminal-exit'
/** Event fired when the transport or the remote session fails. */
export const ANTH_TERMINAL_ERROR_EVENT = 'anth-terminal-error'
/** Event fired on every connection state transition. */
export const ANTH_TERMINAL_STATUS_EVENT = 'anth-terminal-status'

export interface AnthTerminalReadyDetail {
  program: string
  size: TerminalSize
}

export interface AnthTerminalExitDetail {
  code: number
}

export interface AnthTerminalErrorDetail {
  message: string
}

export interface AnthTerminalStatusDetail {
  status: TerminalStatus
}

const DEFAULT_FONT =
  'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", monospace'

const DEFAULT_THEME: ITheme = {
  background: '#0b0f14',
  foreground: '#d7e2ee',
  cursor: '#7dd3fc',
  cursorAccent: '#0b0f14',
  selectionBackground: '#2b4a5f',
  black: '#1b2430',
  red: '#f87171',
  green: '#6ee7b7',
  yellow: '#fbbf24',
  blue: '#7dd3fc',
  magenta: '#c084fc',
  cyan: '#67e8f9',
  white: '#d7e2ee',
  brightBlack: '#64748b',
  brightRed: '#fca5a5',
  brightGreen: '#86efac',
  brightYellow: '#fde68a',
  brightBlue: '#bae6fd',
  brightMagenta: '#d8b4fe',
  brightCyan: '#a5f3fc',
  brightWhite: '#f8fafc',
}

function statusLabel(status: TerminalStatus): string {
  switch (status) {
    case 'idle':
      return 'idle'
    case 'connecting':
      return 'connecting'
    case 'open':
      return 'connected'
    case 'closed':
      return 'closed'
    case 'error':
      return 'error'
  }
}

function commonPrefix(values: string[]): string {
  let prefix = values[0] ?? ''
  for (const value of values.slice(1)) {
    while (prefix !== '' && !value.startsWith(prefix)) {
      prefix = prefix.slice(0, -1)
    }
  }
  return prefix
}

/**
 * A reusable terminal window backed by xterm.js.
 *
 * The element owns rendering, keyboard input and resizing. It knows nothing
 * about WebSockets or the panel API: supply any {@link TerminalTransport}
 * through the `transport` property and the component connects through it.
 *
 * Up/Down walk a local command history while the normal screen buffer is
 * active. Full-screen programs switch to the alternate buffer, where their
 * arrow keys pass through untouched.
 *
 * @fires anth-terminal-ready - `{ program, size }` after the shell is ready.
 * @fires anth-terminal-exit - `{ code }` when the remote process ends.
 * @fires anth-terminal-error - `{ message }` when the transport fails.
 * @fires anth-terminal-status - `{ status }` on every state transition.
 *
 * @slot title - Replaces the title text.
 * @slot actions - Extra controls placed at the right of the title bar.
 *
 * @csspart window - The outer window frame.
 * @csspart titlebar - The title bar.
 * @csspart glyph - The `>_` prompt mark.
 * @csspart title - The title text container.
 * @csspart actions - The title bar action container.
 * @csspart screen - The terminal screen.
 * @csspart terminal - The xterm mount point.
 * @csspart status - The connection status badge.
 *
 * @cssprop --terminal-height - Window height. Defaults to `22rem`.
 * @cssprop --terminal-bg - Screen background.
 * @cssprop --terminal-surface - Title bar background.
 * @cssprop --terminal-fg - Default text colour.
 * @cssprop --terminal-muted - Title and secondary text colour.
 * @cssprop --terminal-accent - Caret and emphasis colour.
 * @cssprop --terminal-border - Border and divider colour.
 * @cssprop --terminal-shadow - Window shadow.
 * @cssprop --terminal-radius - Window corner radius.
 * @cssprop --terminal-font - Chrome font family.
 */
@customElement('anth-terminal')
export class AnthTerminal extends LitElement {
  /** Text shown in the title bar when no `title` slot is provided. */
  @property({ type: String })
  title = 'Terminal'

  /** Transport that connects this window to a shell. */
  @property({ attribute: false })
  transport?: TerminalTransport

  /** Connect automatically when a transport is present. */
  @property({ type: Boolean, attribute: 'auto-connect' })
  autoConnect = true

  /** Disable keyboard input while still showing output. */
  @property({ type: Boolean, attribute: 'read-only', reflect: true })
  readOnly = false

  /** Focus xterm's input when the terminal becomes ready. */
  @property({ type: Boolean, attribute: 'auto-focus' })
  autoFocus = false

  /**
   * Enable Up/Down command history on the normal screen buffer. Full-screen
   * programs (btop, vim, htop) use the alternate buffer and keep their arrows.
   */
  @property({ type: Boolean, attribute: 'command-history' })
  commandHistoryEnabled = true

  /** Number of commands remembered for Up/Down. `0` disables storage. */
  @property({ type: Number, attribute: 'history-limit' })
  historyLimit = 100

  /** Complete the current command from local history with Tab. */
  @property({ type: Boolean, attribute: 'tab-completion' })
  tabCompletionEnabled = true

  /** Scrollback in lines. `0` keeps only the visible screen. */
  @property({ type: Number, attribute: 'max-lines' })
  maxLines = 5000

  /** xterm font size in pixels. */
  @property({ type: Number, attribute: 'font-size' })
  fontSize = 13

  /** xterm font family. Empty uses the built-in monospace stack. */
  @property({ type: String, attribute: 'font-family' })
  fontFamily = ''

  /** xterm colour theme. */
  @property({ attribute: false })
  theme?: ITheme

  /** Extra xterm options, applied after the properties above. */
  @property({ attribute: false })
  terminalOptions?: Partial<ITerminalOptions>

  @state()
  private connectionStatus: TerminalStatus = 'idle'

  @state()
  private sessionInfo?: TerminalSessionInfo

  @state()
  private errorMessage = ''

  @state()
  private loading = true

  @query('.terminal-host')
  private terminalHost!: HTMLElement

  private terminal?: Terminal
  private fitAddon?: FitAddon
  private connectedTransport?: TerminalTransport
  private resizeObserver?: ResizeObserver
  private resizeFrame?: number
  private setupPromise?: Promise<void>
  private generation = 0
  private pendingWrites: (string | Uint8Array)[] = []
  private history: string[] = []
  private historyIndex = -1
  private historyDraft = ''
  private currentLine = ''
  private completion?: {
    matches: string[]
    index: number
  }

  /** Current connection status. */
  get status(): TerminalStatus {
    return this.connectionStatus
  }

  /** Session reported by the last successful `ready` frame. */
  get session(): TerminalSessionInfo | undefined {
    return this.sessionInfo
  }

  /** Commands remembered for Up/Down, oldest first. */
  get commandHistory(): readonly string[] {
    return this.history
  }

  /** Forget the local command history and any in-progress line. */
  clearHistory(): void {
    this.history = []
    this.historyIndex = -1
    this.historyDraft = ''
    this.currentLine = ''
    this.completion = undefined
  }

  override connectedCallback(): void {
    super.connectedCallback()
    if (this.hasUpdated) {
      void this.ensureTerminal()
    }
  }

  override disconnectedCallback(): void {
    super.disconnectedCallback()
    this.generation += 1
    this.setupPromise = undefined
    this.disconnect()
    this.resizeObserver?.disconnect()
    this.resizeObserver = undefined
    if (this.resizeFrame !== undefined) {
      cancelAnimationFrame(this.resizeFrame)
      this.resizeFrame = undefined
    }
    this.terminal?.dispose()
    this.terminal = undefined
    this.fitAddon = undefined
  }

  protected override firstUpdated(): void {
    void this.ensureTerminal().then(() => {
      if (this.autoFocus) this.focus()
    })
  }

  protected override updated(changed: PropertyValues<this>): void {
    if (
      changed.has('readOnly') ||
      changed.has('fontSize') ||
      changed.has('fontFamily') ||
      changed.has('maxLines') ||
      changed.has('theme') ||
      changed.has('terminalOptions')
    ) {
      this.applyOptions()
    }

    if (
      changed.has('transport') ||
      (changed.has('autoConnect') && this.autoConnect)
    ) {
      if (!this.terminal) return
      if (this.transport === this.connectedTransport) return

      this.disconnect()
      if (this.autoConnect && this.transport) {
        void this.connect()
      }
    }
  }

  /** Writes local output into the terminal, independent from the transport. */
  write(data: string | Uint8Array): void {
    if (this.terminal) {
      this.terminal.write(data)
    } else {
      this.pendingWrites.push(data)
    }
  }

  /** Convenience wrapper that appends a CRLF. */
  writeln(text = ''): void {
    this.write(`${text}\r\n`)
  }

  clear(): void {
    this.terminal?.clear()
  }

  reset(): void {
    this.terminal?.reset()
  }

  focus(): void {
    this.terminal?.focus()
  }

  /** Recomputes the character grid from the container size. */
  fit(): void {
    const host = this.terminalHost
    if (!this.terminal || !this.fitAddon || !host) return
    if (host.clientWidth === 0 || host.clientHeight === 0) return

    try {
      this.fitAddon.fit()
    } catch {
      // A hidden or zero-sized container is not an error.
    }
  }

  /** Connects the supplied transport, or the one already assigned. */
  async connect(transport: TerminalTransport | undefined = this.transport) {
    if (!transport) return

    await this.ensureTerminal()
    if (!this.terminal) return

    this.disconnect()
    this.connectedTransport = transport
    this.errorMessage = ''
    this.setStatus('connecting')
    this.fit()

    try {
      const info = await transport.open(this.currentSize(), {
        onData: (data) => this.terminal?.write(data),
        onExit: (code) => this.handleExit(code),
        onError: (error) => this.handleTransportError(error),
        onStatus: (status) => this.setStatus(status),
      })
      this.sessionInfo = info
      this.setStatus('open')
      this.fit()
      // The container may have changed size while the handshake was in flight.
      this.connectedTransport?.resize(this.currentSize())
      this.dispatchEvent(
        new CustomEvent<AnthTerminalReadyDetail>(ANTH_TERMINAL_READY_EVENT, {
          detail: { program: info.program, size: info.size },
          bubbles: true,
          composed: true,
        }),
      )
      if (this.autoFocus) this.focus()
    } catch (error) {
      this.handleTransportError(
        error instanceof Error ? error : new Error(String(error)),
      )
    }
  }

  /** Closes the current transport and leaves the scrollback on screen. */
  disconnect(): void {
    const transport = this.connectedTransport
    this.connectedTransport = undefined
    this.sessionInfo = undefined
    this.completion = undefined
    if (transport) {
      transport.close()
    }
    if (this.connectionStatus !== 'idle') {
      this.setStatus('closed')
    }
  }

  override render() {
    return html`
      <section class="window" part="window">
        <header class="titlebar" part="titlebar">
          <span class="glyph" part="glyph" aria-hidden="true">&gt;_</span>
          <span class="title" part="title">
            <slot name="title">${this.title}</slot>
          </span>
          <span class="actions" part="actions">
            <slot name="actions"></slot>
            <span
              class="status"
              part="status"
              data-status=${this.connectionStatus}
              title=${this.statusTitle()}
            >
              <span class="status-dot" aria-hidden="true"></span>
              ${statusLabel(this.connectionStatus)}
            </span>
          </span>
        </header>

        <div class="screen" part="screen">
          <div class="terminal-host" part="terminal"></div>
          ${this.loading
            ? html`<div class="loading">Loading terminal...</div>`
            : nothing}
          ${!this.loading && !this.terminal && this.errorMessage !== ''
            ? html`<div class="error" role="alert">${this.errorMessage}</div>`
            : nothing}
        </div>
      </section>
    `
  }

  static styles = [
    unsafeCSS(xtermCss),
    css`
      :host {
        --terminal-height: min(44rem, 82vh);
        --terminal-bg: #0b0f14;
        --terminal-surface: #131a23;
        --terminal-fg: #d7e2ee;
        --terminal-muted: #7f90a3;
        --terminal-accent: #7dd3fc;
        --terminal-border: #263241;
        --terminal-shadow: 0 18px 50px rgba(0, 0, 0, 0.45);
        --terminal-radius: 12px;
        --terminal-font:
          ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas,
          'Liberation Mono', monospace;

        display: block;
        height: var(--terminal-height);
        min-height: 6rem;
        min-width: 0;
        color-scheme: dark;
        font-family: var(--terminal-font);
        font-size: 13px;
        -webkit-font-smoothing: antialiased;
        -moz-osx-font-smoothing: grayscale;
      }

      * {
        box-sizing: border-box;
      }

      .window {
        display: flex;
        flex-direction: column;
        height: 100%;
        min-height: 0;
        overflow: hidden;
        color: var(--terminal-fg);
        background: var(--terminal-bg);
        border: 1px solid var(--terminal-border);
        border-radius: var(--terminal-radius);
        box-shadow: var(--terminal-shadow);
      }

      .titlebar {
        display: flex;
        flex: 0 0 auto;
        align-items: center;
        gap: 0.75rem;
        padding: 0.55rem 0.8rem;
        background:
          linear-gradient(
            180deg,
            rgba(255, 255, 255, 0.05),
            rgba(255, 255, 255, 0)
          ),
          var(--terminal-surface);
        border-bottom: 1px solid var(--terminal-border);
        user-select: none;
      }

      .glyph {
        flex: 0 0 auto;
        color: var(--terminal-prompt);
        font-size: 0.95em;
        font-weight: 700;
        letter-spacing: 0.02em;
        line-height: 1;
        white-space: pre;
      }

      .title {
        flex: 1 1 auto;
        min-width: 0;
        overflow: hidden;
        color: var(--terminal-muted);
        font-size: 0.72em;
        font-weight: 600;
        letter-spacing: 0.1em;
        text-align: center;
        text-overflow: ellipsis;
        text-transform: uppercase;
        white-space: nowrap;
      }

      .actions {
        display: flex;
        flex: 0 0 auto;
        align-items: center;
        gap: 0.5rem;
        color: var(--terminal-muted);
      }

      .status {
        display: inline-flex;
        align-items: center;
        gap: 0.32rem;
        padding: 0.1rem 0.42rem;
        color: var(--terminal-muted);
        font-size: 0.64em;
        letter-spacing: 0.04em;
        text-transform: lowercase;
        border: 1px solid var(--terminal-border);
        border-radius: 999px;
        background: rgba(255, 255, 255, 0.03);
        white-space: nowrap;
      }

      .status-dot {
        width: 0.38rem;
        height: 0.38rem;
        border-radius: 50%;
        background: var(--terminal-muted);
      }

      .status[data-status='connecting'] .status-dot {
        background: #fbbf24;
      }

      .status[data-status='open'] .status-dot {
        background: #34d399;
      }

      .status[data-status='error'] .status-dot {
        background: #f87171;
      }

      .screen {
        position: relative;
        display: flex;
        flex: 1 1 auto;
        flex-direction: column;
        min-height: 0;
        padding: 0.62rem 0.8rem 0.55rem;
        background:
          radial-gradient(
            120% 90% at 50% 0%,
            rgba(125, 211, 252, 0.06),
            transparent 55%
          ),
          var(--terminal-bg);
      }

      .terminal-host {
        flex: 1 1 auto;
        min-height: 0;
        overflow: hidden;
      }

      .terminal-host .xterm {
        height: 100%;
      }

      .loading {
        position: absolute;
        inset: 0;
        display: grid;
        place-items: center;
        color: var(--terminal-muted);
        font-size: 0.8em;
        pointer-events: none;
      }

      .error {
        position: absolute;
        inset: 0;
        display: grid;
        place-items: center;
        padding: 1rem;
        color: #fca5a5;
        font-size: 0.8em;
        text-align: center;
        background: var(--terminal-bg);
      }
    `,
  ]

  private async ensureTerminal(): Promise<void> {
    if (this.terminal) return
    if (!this.isConnected) return
    if (!this.hasUpdated) {
      await this.updateComplete
    }
    if (!this.terminalHost) return

    if (!this.setupPromise) {
      const setup = this.setupTerminal()
      this.setupPromise = setup
      void setup.finally(() => {
        if (this.setupPromise === setup) {
          this.setupPromise = undefined
        }
      })
    }
    return this.setupPromise
  }

  private async setupTerminal(): Promise<void> {
    const generation = ++this.generation

    try {
      const [{ Terminal }, { FitAddon }] = await Promise.all([
        import('@xterm/xterm'),
        import('@xterm/addon-fit'),
      ])
      if (generation !== this.generation || !this.isConnected) return

      const terminal = new Terminal({
        convertEol: false,
        cursorBlink: true,
        fontFamily: this.fontFamily || DEFAULT_FONT,
        fontSize: this.fontSize,
        lineHeight: 1.35,
        scrollback: this.maxLines,
        theme: this.theme ?? DEFAULT_THEME,
        ...this.terminalOptions,
      })
      const fitAddon = new FitAddon()
      terminal.loadAddon(fitAddon)
      terminal.open(this.terminalHost)

      this.terminal = terminal
      this.fitAddon = fitAddon
      this.loading = false
      this.applyOptions()
      this.flushPendingWrites()

      terminal.onData((data) => this.handleInput(data))
      terminal.onResize((size) => this.handleResize(size))
      terminal.attachCustomKeyEventHandler((event) => this.handleKeyEvent(event))
      terminal.buffer.onBufferChange(() => this.resetHistoryNavigation())

      this.observeResize()
      requestAnimationFrame(() => this.fit())

      if (this.autoFocus) this.focus()
      if (this.autoConnect && this.transport) {
        void this.connect()
      }
    } catch (error) {
      if (generation !== this.generation) return
      this.loading = false
      this.handleTransportError(
        error instanceof Error ? error : new Error(String(error)),
      )
    }
  }

  private observeResize(): void {
    if (typeof ResizeObserver === 'undefined' || !this.terminalHost) return

    this.resizeObserver?.disconnect()
    this.resizeObserver = new ResizeObserver(() => {
      if (this.resizeFrame !== undefined) {
        cancelAnimationFrame(this.resizeFrame)
      }
      this.resizeFrame = requestAnimationFrame(() => {
        this.resizeFrame = undefined
        this.fit()
      })
    })
    this.resizeObserver.observe(this.terminalHost)
  }

  private applyOptions(): void {
    const terminal = this.terminal
    if (!terminal) return

    terminal.options.fontSize = this.fontSize
    terminal.options.fontFamily = this.fontFamily || DEFAULT_FONT
    terminal.options.scrollback = this.maxLines
    terminal.options.disableStdin = this.readOnly
    terminal.options.theme = this.theme ?? DEFAULT_THEME
    if (this.terminalOptions) {
      Object.assign(terminal.options, this.terminalOptions)
    }

    this.fit()
    if (this.readOnly) {
      terminal.blur()
    }
  }

  private handleInput(data: string): void {
    if (this.readOnly || this.connectionStatus !== 'open') return
    if (this.commandHistoryEnabled) {
      this.trackCommandLine(data)
    }
    this.connectedTransport?.sendInput(new TextEncoder().encode(data))
  }

  private handleKeyEvent(event: KeyboardEvent): boolean {
    if (event.type !== 'keydown') return true
    if (event.key === 'Tab') return this.handleTabCompletion(event)
    if (!this.commandHistoryEnabled || this.historyLimit <= 0) return true
    if (event.key !== 'ArrowUp' && event.key !== 'ArrowDown') return true
    if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) {
      return true
    }
    if (this.readOnly || this.connectionStatus !== 'open') return true
    if (!this.normalBufferActive() || this.history.length === 0) return true
    if (this.terminal?.modes.applicationCursorKeysMode) return true
    if (!this.navigateHistory(event.key === 'ArrowUp' ? -1 : 1)) return true

    event.preventDefault()
    return false
  }

  private handleTabCompletion(event: KeyboardEvent): boolean {
    if (!this.tabCompletionEnabled) return true
    if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) {
      return true
    }
    if (this.readOnly || this.connectionStatus !== 'open') return true
    if (!this.normalBufferActive()) return true
    if (this.terminal?.modes.applicationCursorKeysMode) return true

    if (this.completion) {
      const matches = this.completion.matches
      this.completion.index = (this.completion.index + 1) % matches.length
      this.replaceCurrentLine(matches[this.completion.index] ?? '')
      event.preventDefault()
      return false
    }

    const prefix = this.currentLine
    if (prefix.trim() === '') return true

    const matches = this.historyMatches(prefix)
    if (matches.length === 0) return true

    if (matches.length === 1) {
      this.replaceCurrentLine(matches[0] ?? '')
      this.completion = { matches, index: 0 }
    } else {
      const common = commonPrefix(matches)
      if (common.length > prefix.length) {
        this.replaceCurrentLine(common)
        this.completion = { matches, index: -1 }
      } else {
        this.replaceCurrentLine(matches[0] ?? '')
        this.completion = { matches, index: 0 }
      }
    }

    event.preventDefault()
    return false
  }

  private historyMatches(prefix: string): string[] {
    const seen = new Set<string>()
    const matches: string[] = []

    // Newest first, so repeated Tab cycles through the most useful entries.
    for (let i = this.history.length - 1; i >= 0; i--) {
      const command = this.history[i]
      if (!command.startsWith(prefix) || seen.has(command)) continue
      seen.add(command)
      matches.push(command)
    }

    return matches
  }

  /** Tracks the line the user is typing without touching the shell. */
  private trackCommandLine(data: string): void {
    if (
      !this.normalBufferActive() ||
      this.terminal?.modes.applicationCursorKeysMode
    ) {
      return
    }

    this.completion = undefined

    // Left/right/home/end arrive as ANSI sequences. They do not change the
    // command text we remember; stripping them keeps the history from
    // accumulating garbage like `[D`.
    const plain = data.replace(/\x1b(?:\[[0-9;?]*[ -/]*[@-~]|O[A-Za-z]|.)/g, '')
    for (const char of plain) {
      if (char === '\r' || char === '\n') {
        this.rememberCommand()
        continue
      }
      if (char === '\u007f' || char === '\b') {
        const chars = Array.from(this.currentLine)
        chars.pop()
        this.currentLine = chars.join('')
        continue
      }
      if (char === '\u0003' || char === '\u0015' || char === '\u000c') {
        this.resetHistoryNavigation()
        continue
      }
      if (char >= ' ') {
        this.currentLine += char
      }
    }
  }

  private rememberCommand(): void {
    const line = this.currentLine
    this.resetHistoryNavigation()
    if (line.trim() === '' || this.historyLimit <= 0) return

    this.history = [...this.history, line].slice(-this.historyLimit)
  }

  private navigateHistory(direction: -1 | 1): boolean {
    this.completion = undefined

    if (direction === -1) {
      if (this.historyIndex === -1) {
        this.historyDraft = this.currentLine
        this.historyIndex = this.history.length - 1
      } else if (this.historyIndex > 0) {
        this.historyIndex -= 1
      }

      this.replaceCurrentLine(this.history[this.historyIndex] ?? '')
      return true
    }

    if (this.historyIndex === -1) return false

    if (this.historyIndex < this.history.length - 1) {
      this.historyIndex += 1
      this.replaceCurrentLine(this.history[this.historyIndex] ?? '')
    } else {
      this.historyIndex = -1
      const draft = this.historyDraft
      this.historyDraft = ''
      this.replaceCurrentLine(draft)
    }
    return true
  }

  private replaceCurrentLine(next: string): void {
    // Ctrl-U kills the canonical input line (VKILL) and also works in common
    // line editors, so the shell replaces the visible command with the one
    // selected from history.
    this.connectedTransport?.sendInput(
      new TextEncoder().encode(`\u0015${next}`),
    )
    this.currentLine = next
  }

  private resetHistoryNavigation(): void {
    this.historyIndex = -1
    this.historyDraft = ''
    this.currentLine = ''
    this.completion = undefined
  }

  private normalBufferActive(): boolean {
    return this.terminal?.buffer.active.type === 'normal'
  }

  private handleResize(size: TerminalSize): void {
    if (this.connectionStatus !== 'open') return
    this.connectedTransport?.resize(size)
  }

  private handleExit(code: number): void {
    this.write(
      `\r\n\x1b[38;5;245m[process exited with code ${code}]\x1b[0m\r\n`,
    )
    this.sessionInfo = undefined
    this.setStatus('closed')
    this.dispatchEvent(
      new CustomEvent<AnthTerminalExitDetail>(ANTH_TERMINAL_EXIT_EVENT, {
        detail: { code },
        bubbles: true,
        composed: true,
      }),
    )
  }

  private handleTransportError(error: Error): void {
    this.errorMessage = error.message
    this.write(`\r\n\x1b[31m[terminal error: ${error.message}]\x1b[0m\r\n`)
    this.setStatus('error')
    this.dispatchEvent(
      new CustomEvent<AnthTerminalErrorDetail>(ANTH_TERMINAL_ERROR_EVENT, {
        detail: { message: error.message },
        bubbles: true,
        composed: true,
      }),
    )
  }

  private setStatus(status: TerminalStatus): void {
    if (this.connectionStatus === status) return
    this.connectionStatus = status
    this.dispatchEvent(
      new CustomEvent<AnthTerminalStatusDetail>(ANTH_TERMINAL_STATUS_EVENT, {
        detail: { status },
        bubbles: true,
        composed: true,
      }),
    )
  }

  private currentSize(): TerminalSize {
    const terminal = this.terminal
    return {
      cols: terminal?.cols ?? 80,
      rows: terminal?.rows ?? 24,
    }
  }

  private flushPendingWrites(): void {
    if (!this.terminal) return
    for (const data of this.pendingWrites) {
      this.terminal.write(data)
    }
    this.pendingWrites = []
  }

  private statusTitle(): string {
    if (this.errorMessage !== '') return this.errorMessage
    if (this.sessionInfo) {
      return `${this.sessionInfo.program} · ${this.sessionInfo.size.cols}x${this.sessionInfo.size.rows}`
    }
    return statusLabel(this.connectionStatus)
  }
}

declare global {
  interface HTMLElementTagNameMap {
    'anth-terminal': AnthTerminal
  }

  interface HTMLElementEventMap {
    'anth-terminal-ready': CustomEvent<AnthTerminalReadyDetail>
    'anth-terminal-exit': CustomEvent<AnthTerminalExitDetail>
    'anth-terminal-error': CustomEvent<AnthTerminalErrorDetail>
    'anth-terminal-status': CustomEvent<AnthTerminalStatusDetail>
  }
}
