import { LitElement, css, html, nothing } from 'lit'
import type { PropertyValues } from 'lit'
import { customElement, property, state } from 'lit/decorators.js'
import './anth-process-table'
import type { AnthProcessSelectDetail, ProcessView } from './anth-process-table'
import type {
  ProcessClient,
  ProcessConnectionStatus,
  ProcessRow,
  ProcessSignal,
  ProcessSnapshot,
} from '../lib/processes/process-client'

export const ANTH_PROCESS_SIGNAL_EVENT = 'anth-process-signal'

export interface AnthProcessSignalDetail {
  pid: number
  signal: ProcessSignal
}

const SIGNALS: ProcessSignal[] = [
  'term',
  'kill',
  'hup',
  'int',
  'quit',
  'stop',
  'cont',
]

/**
 * Connected process monitor: fetches snapshots, follows the SSE stream and
 * computes CPU percentages from consecutive tick counts.
 *
 * The component depends on the injected `ProcessClient`, not on `/api`.
 *
 * @fires anth-process-signal - `{ pid, signal }` after a successful signal.
 */
@customElement('anth-process-monitor')
export class AnthProcessMonitor extends LitElement {
  @property({ attribute: false })
  client?: ProcessClient

  /** SSE interval in milliseconds. */
  @property({ type: Number, attribute: 'refresh-interval' })
  refreshInterval = 2000

  @property({ type: Boolean, attribute: 'auto-start' })
  autoStart = true

  @state()
  private rows: ProcessRow[] = []

  @state()
  private status: ProcessConnectionStatus = 'idle'

  @state()
  private controlEnabled = false

  @state()
  private warning = ''

  @state()
  private errorMessage = ''

  @state()
  private filter = ''

  @state()
  private view: ProcessView = 'flat'

  @state()
  private selectedPid?: number

  @state()
  private pendingSignal: ProcessSignal = 'term'

  @state()
  private busy = false

  @state()
  private total = 0

  private previous?: ProcessSnapshot
  private unsubscribe?: () => void
  private generation = 0

  override disconnectedCallback(): void {
    super.disconnectedCallback()
    this.stop()
  }

  protected override willUpdate(changed: PropertyValues<this>): void {
    if (
      (changed.has('client') ||
        changed.has('refreshInterval') ||
        (changed.has('autoStart') && this.autoStart)) &&
      this.client &&
      this.autoStart
    ) {
      this.start()
    }
  }

  /** Starts the initial fetch and the live SSE stream. */
  start(): void {
    this.stop()
    if (!this.client) {
      this.status = 'idle'
      return
    }

    const generation = ++this.generation
    this.previous = undefined
    this.errorMessage = ''
    this.status = 'connecting'

    this.unsubscribe = this.client.watch(this.refreshInterval, {
      onSnapshot: (snapshot) => {
        if (generation === this.generation) this.applySnapshot(snapshot)
      },
      onError: (error) => {
        if (generation !== this.generation) return
        this.errorMessage = error.message
        if (this.status !== 'live') this.status = 'error'
      },
      onStatus: (status) => {
        if (generation !== this.generation) return
        this.status = status
        if (status === 'live') this.errorMessage = ''
      },
    })

    void this.refreshSnapshot(generation)
  }

  /** Stops the SSE stream. Existing rows stay visible. */
  stop(): void {
    this.generation += 1
    this.unsubscribe?.()
    this.unsubscribe = undefined
    this.previous = undefined
    if (this.status !== 'idle') this.status = 'closed'
  }

  /** Fetches one snapshot immediately, without restarting the stream. */
  refresh(): void {
    void this.refreshSnapshot(this.generation)
  }

  private async refreshSnapshot(generation: number): Promise<void> {
    if (!this.client) return

    try {
      const snapshot = await this.client.list()
      if (generation === this.generation) this.applySnapshot(snapshot)
    } catch (error) {
      if (generation !== this.generation) return
      this.errorMessage =
        error instanceof Error ? error.message : String(error)
      this.status = 'error'
    }
  }

  private applySnapshot(snapshot: ProcessSnapshot): void {
    const previous = this.previous
    const previousByKey = new Map<string, number>()
    const elapsed =
      previous === undefined
        ? 0
        : Math.max(0, (snapshot.timestamp_ms - previous.timestamp_ms) / 1000)

    if (previous) {
      for (const process of previous.processes) {
        previousByKey.set(processKey(process.pid, process.start_ticks), process.cpu_ticks)
      }
    }

    this.rows = snapshot.processes.map((process) => {
      const previousTicks = previousByKey.get(
        processKey(process.pid, process.start_ticks),
      )

      let cpuPercent: number | null = null
      if (
        previousTicks !== undefined &&
        elapsed > 0 &&
        snapshot.clock_ticks > 0 &&
        process.cpu_ticks >= previousTicks
      ) {
        cpuPercent =
          ((process.cpu_ticks - previousTicks) /
            snapshot.clock_ticks /
            elapsed) *
          100
      }

      return { ...process, cpu_percent: cpuPercent }
    })

    this.previous = snapshot
    this.total = snapshot.total
    this.controlEnabled = snapshot.control_enabled
    this.warning =
      snapshot.warning ??
      (snapshot.processes.length < snapshot.total
        ? `showing ${snapshot.processes.length} of ${snapshot.total} processes`
        : '')

    if (
      this.selectedPid !== undefined &&
      !this.rows.some((process) => process.pid === this.selectedPid)
    ) {
      this.selectedPid = undefined
    }
  }

  private get selectedProcess(): ProcessRow | undefined {
    return this.rows.find((process) => process.pid === this.selectedPid)
  }

  private get filteredRows(): ProcessRow[] {
    const needle = this.filter.trim().toLowerCase()
    if (needle === '') return this.rows

    return this.rows.filter((process) =>
      [
        String(process.pid),
        process.user,
        process.name,
        process.command,
        process.state,
      ].some((value) => value.toLowerCase().includes(needle)),
    )
  }

  private statusLabel(): string {
    switch (this.status) {
      case 'idle':
        return 'idle'
      case 'connecting':
        return 'connecting'
      case 'live':
        return 'live'
      case 'error':
        return 'error'
      case 'closed':
        return 'closed'
    }
  }

  private onFilterInput(event: Event): void {
    this.filter = (event.target as HTMLInputElement).value
  }

  private onSelect(event: CustomEvent<AnthProcessSelectDetail>): void {
    this.selectedPid = event.detail.pid
  }

  private onSignalChange(event: Event): void {
    this.pendingSignal = (event.target as HTMLSelectElement)
      .value as ProcessSignal
  }

  private async sendSignal(): Promise<void> {
    const process = this.selectedProcess
    if (!this.client || !process || !this.controlEnabled || this.busy) return

    const label = process.command === '' ? process.name : process.command
    if (
      !window.confirm(
        `Send ${this.pendingSignal.toUpperCase()} to PID ${process.pid} (${label})?`,
      )
    ) {
      return
    }

    this.busy = true
    this.errorMessage = ''
    try {
      await this.client.signal(process.pid, this.pendingSignal)
      this.dispatchEvent(
        new CustomEvent<AnthProcessSignalDetail>(ANTH_PROCESS_SIGNAL_EVENT, {
          detail: { pid: process.pid, signal: this.pendingSignal },
          bubbles: true,
          composed: true,
        }),
      )
      this.refresh()
    } catch (error) {
      this.errorMessage =
        error instanceof Error ? error.message : String(error)
    } finally {
      this.busy = false
    }
  }

  override render() {
    const selected = this.selectedProcess

    return html`
      <section class="monitor" part="monitor">
        <header class="toolbar" part="toolbar">
          <span class="status" part="status" data-status=${this.status}>
            <span class="dot" aria-hidden="true"></span>
            ${this.statusLabel()}
          </span>
          <span class="count">
            ${this.filteredRows.length} / ${this.total} processes
          </span>
          <span class="spacer"></span>
          <div class="view-switch" role="group" aria-label="Process view">
            <button
              type="button"
              class=${this.view === 'flat' ? 'active' : ''}
              @click=${() => (this.view = 'flat')}
            >
              Flat
            </button>
            <button
              type="button"
              class=${this.view === 'tree' ? 'active' : ''}
              @click=${() => (this.view = 'tree')}
            >
              Tree
            </button>
          </div>
          <button type="button" @click=${this.refresh} ?disabled=${!this.client}>
            Refresh
          </button>
          <button
            type="button"
            @click=${this.status === 'live' || this.status === 'connecting'
              ? this.stop
              : this.start}
            ?disabled=${!this.client}
          >
            ${this.status === 'live' || this.status === 'connecting'
              ? 'Pause'
              : 'Resume'}
          </button>
        </header>

        ${this.errorMessage === ''
          ? nothing
          : html`
              <p class="error" part="error" role="alert">${this.errorMessage}</p>
            `}

        <div class="filters" part="filters">
          <input
            type="search"
            .value=${this.filter}
            placeholder="Filter by pid, user, name or command"
            aria-label="Filter processes"
            @input=${this.onFilterInput}
          />
        </div>

        <anth-process-table
          .processes=${this.filteredRows}
          .selectedPid=${this.selectedPid}
          .warning=${this.warning}
          .view=${this.view}
          .emptyMessage=${this.client
            ? 'No processes match the current filter.'
            : 'No process client configured.'}
          @anth-process-select=${this.onSelect}
        ></anth-process-table>

        <footer class="actions" part="actions">
          <div class="selection">
            ${selected
              ? html`
                  <span class="pid">PID ${selected.pid}</span>
                  <span class="muted">${selected.user}</span>
                  <span class="command" title=${selected.command}>
                    ${selected.command}
                  </span>
                `
              : html`<span class="muted">Select a process to send a signal.</span>`}
          </div>

          ${this.controlEnabled
            ? html`
                <div class="signal">
                  <select
                    .value=${this.pendingSignal}
                    aria-label="Signal"
                    @change=${this.onSignalChange}
                  >
                    ${SIGNALS.map(
                      (signal) => html`<option value=${signal}>
                        ${signal.toUpperCase()}
                      </option>`,
                    )}
                  </select>
                  <button
                    type="button"
                    class="danger"
                    ?disabled=${!selected || this.busy}
                    @click=${this.sendSignal}
                  >
                    ${this.busy ? 'Sending...' : 'Send signal'}
                  </button>
                </div>
              `
            : html`
                <span class="muted control-off">
                  Control is disabled by the daemon.
                </span>
              `}
        </footer>
      </section>
    `
  }

  static styles = css`
    :host {
      --process-bg: #0b0f14;
      --process-surface: #131a23;
      --process-fg: #d7e2ee;
      --process-muted: #7f90a3;
      --process-accent: #7dd3fc;
      --process-border: #263241;
      display: block;
      min-width: 0;
      color: var(--process-fg);
      font: 13px/1.45 var(
          --process-font,
          ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace
        );
    }

    * {
      box-sizing: border-box;
    }

    .monitor {
      display: flex;
      flex-direction: column;
      gap: 0.7rem;
      padding: 0.9rem;
      background: var(--process-bg);
      border: 1px solid var(--process-border);
      border-radius: 12px;
    }

    .toolbar,
    .filters,
    .actions {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: 0.55rem;
    }

    .toolbar {
      color: var(--process-muted);
    }

    .spacer {
      flex: 1 1 auto;
    }

    .status {
      display: inline-flex;
      align-items: center;
      gap: 0.35rem;
      padding: 0.12rem 0.5rem;
      color: var(--process-muted);
      font-size: 0.82em;
      border: 1px solid var(--process-border);
      border-radius: 999px;
      text-transform: lowercase;
    }

    .status .dot {
      width: 0.42rem;
      height: 0.42rem;
      border-radius: 50%;
      background: var(--process-muted);
    }

    .status[data-status='live'] .dot {
      background: #34d399;
    }

    .status[data-status='connecting'] .dot {
      background: #fbbf24;
    }

    .status[data-status='error'] .dot {
      background: #f87171;
    }

    .count {
      font-size: 0.85em;
    }

    button,
    select,
    input {
      padding: 0.42rem 0.6rem;
      color: var(--process-fg);
      font: inherit;
      font-size: 0.92em;
      background: var(--process-surface);
      border: 1px solid var(--process-border);
      border-radius: 7px;
    }

    input {
      flex: 1 1 16rem;
      min-width: 0;
    }

    button {
      cursor: pointer;
    }

    button:hover:not(:disabled) {
      border-color: rgba(125, 211, 252, 0.55);
    }

    button:focus-visible,
    select:focus-visible,
    input:focus-visible {
      outline: 2px solid var(--process-accent);
      outline-offset: 2px;
    }

    button:disabled {
      cursor: not-allowed;
      opacity: 0.45;
    }

    button.danger {
      color: #fecaca;
      border-color: rgba(248, 113, 113, 0.45);
      background: rgba(248, 113, 113, 0.1);
    }

    .view-switch {
      display: inline-flex;
      overflow: hidden;
      border: 1px solid var(--process-border);
      border-radius: 7px;
    }

    .view-switch button {
      border: 0;
      border-radius: 0;
      background: transparent;
    }

    .view-switch button + button {
      border-left: 1px solid var(--process-border);
    }

    .view-switch button.active {
      color: var(--process-bg);
      background: var(--process-accent);
    }

    .error {
      margin: 0;
      padding: 0.5rem 0.7rem;
      color: #fecaca;
      border: 1px solid rgba(248, 113, 113, 0.35);
      border-radius: 7px;
      background: rgba(248, 113, 113, 0.08);
    }

    .actions {
      justify-content: space-between;
      align-items: center;
      min-height: 2.3rem;
    }

    .selection {
      display: flex;
      flex: 1 1 auto;
      flex-wrap: wrap;
      align-items: center;
      justify-content: flex-start;
      gap: 0.5rem;
      min-width: 0;
      text-align: left;
    }

    .pid {
      color: var(--process-accent);
      font-weight: 600;
    }

    .muted {
      color: var(--process-muted);
    }

    .command {
      max-width: none;
      overflow: visible;
      text-align: left;
      text-overflow: clip;
      white-space: pre-wrap;
      overflow-wrap: anywhere;
    }

    .signal {
      display: flex;
      gap: 0.5rem;
      align-items: center;
    }

    .control-off {
      font-size: 0.85em;
    }
  `
}

function processKey(pid: number, startTicks: number): string {
  return `${pid}:${startTicks}`
}

declare global {
  interface HTMLElementTagNameMap {
    'anth-process-monitor': AnthProcessMonitor
  }

  interface HTMLElementEventMap {
    'anth-process-signal': CustomEvent<AnthProcessSignalDetail>
  }
}
