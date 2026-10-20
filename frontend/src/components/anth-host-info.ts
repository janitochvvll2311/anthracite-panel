import { LitElement, css, html, nothing } from 'lit'
import type { PropertyValues } from 'lit'
import { customElement, property, state } from 'lit/decorators.js'
import type {
  HostInfo,
  HostInfoClient,
  HostInfoConnectionStatus,
} from '../lib/hostinfo/host-info-client'
import {
  formatKib,
  formatLoad,
  formatPercent,
  formatUptime,
  loadPercent,
  memoryUsedKib,
  swapUsedKib,
  usagePercent,
} from '../lib/hostinfo/display'
import { onTabShow } from '../lib/ui/tab-visibility'

const INTERVALS = [1000, 2000, 5000, 10000]

/**
 * Host overview backed by `GET /api/host-info`.
 *
 * It depends on the injected `HostInfoClient`; the HTTP/SSE implementation
 * lives in `lib/hostinfo/host-info-client.ts`.
 */
@customElement('anth-host-info')
export class AnthHostInfo extends LitElement {
  @property({ attribute: false })
  client?: HostInfoClient

  /** SSE interval in milliseconds. */
  @property({ type: Number, attribute: 'refresh-interval' })
  refreshInterval = 2000

  @property({ type: Boolean, attribute: 'auto-start' })
  autoStart = true

  @state()
  private info?: HostInfo

  @state()
  private status: HostInfoConnectionStatus = 'idle'

  @state()
  private watching = false

  @state()
  private loading = false

  @state()
  private errorMessage = ''

  private generation = 0
  private unsubscribe?: () => void
  private offShow?: () => void
  private resumeOnShow = false

  override disconnectedCallback(): void {
    super.disconnectedCallback()
    // Parking must not resurrect a stream the operator paused on purpose, so
    // the decision is taken before `stop()` clears it.
    this.resumeOnShow = this.unsubscribe !== undefined
    this.stop()
    this.offShow?.()
    this.offShow = undefined
  }

  override connectedCallback(): void {
    super.connectedCallback()
    // A tab panel parks a hidden tab with `remove()`, which stops the stream
    // here through `disconnectedCallback`. Coming back changes no property, so
    // `willUpdate` never runs: this listener is the only re-show signal.
    this.offShow = onTabShow(this, () => {
      if (this.resumeOnShow && this.client) this.start()
    })
  }

  protected override willUpdate(changed: PropertyValues<this>): void {
    if (
      this.client &&
      this.autoStart &&
      (changed.has('client') ||
        changed.has('refreshInterval') ||
        (changed.has('autoStart') && this.autoStart))
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
    this.errorMessage = ''
    this.watching = true
    this.status = 'connecting'

    this.unsubscribe = this.client.watch(this.refreshInterval, {
      onSnapshot: (info) => {
        if (generation !== this.generation) return
        this.applyInfo(info)
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

    void this.fetchSnapshot(generation)
  }

  /** Stops the SSE stream. The last snapshot stays visible. */
  stop(): void {
    this.generation += 1
    this.unsubscribe?.()
    this.unsubscribe = undefined
    this.watching = false
    if (this.status !== 'idle') this.status = 'closed'
  }

  /** Fetches one snapshot without restarting the stream. */
  refresh(): void {
    void this.fetchSnapshot(this.generation)
  }

  private toggleWatch(): void {
    if (this.watching) {
      this.stop()
    } else {
      this.start()
    }
  }

  private onIntervalChange(event: Event): void {
    this.refreshInterval = Number(
      (event.currentTarget as HTMLSelectElement).value,
    )
  }

  private intervals(): number[] {
    return [...new Set([...INTERVALS, this.refreshInterval])].sort(
      (a, b) => a - b,
    )
  }

  private async fetchSnapshot(generation: number): Promise<void> {
    if (!this.client) return

    this.loading = true
    try {
      const info = await this.client.get()
      if (generation === this.generation) this.applyInfo(info)
    } catch (error) {
      if (generation !== this.generation) return
      this.errorMessage = messageOf(error)
      this.status = 'error'
    } finally {
      if (generation === this.generation) this.loading = false
    }
  }

  private applyInfo(info: HostInfo): void {
    this.info = info
    this.errorMessage = ''
  }

  private renderStatus() {
    return html`<span
      class="status-pill"
      data-status=${this.status}
      aria-live="polite"
      >${this.status}</span
    >`
  }

  private statusDetail(): string {
    switch (this.status) {
      case 'live':
        return 'live host snapshot'
      case 'connecting':
        return 'connecting to stream'
      case 'error':
        return 'stream error'
      case 'closed':
        return 'stream paused'
      default:
        return 'host snapshot'
    }
  }

  private renderMemory() {
    const memory = this.info?.memory
    if (!memory) return nothing

    const used = memoryUsedKib(memory)
    const percent = usagePercent(used, memory.total_kib)
    return html`
      <article class="card" part="memory">
        <h3>Memory</h3>
        <dl>
          <div><dt>Total</dt><dd>${formatKib(memory.total_kib)}</dd></div>
          <div><dt>Used</dt><dd>${formatKib(used)}</dd></div>
          <div>
            <dt>Available</dt>
            <dd>${formatKib(memory.available_kib)}</dd>
          </div>
        </dl>
        <div class="bar" aria-label=${`Memory used: ${formatPercent(percent)}`}>
          <span style=${`width: ${percent}%`}></span>
        </div>
        <p class="detail">${formatPercent(percent)} used</p>
      </article>
    `
  }

  private renderSwap() {
    const memory = this.info?.memory
    if (!memory) return nothing

    if (memory.swap_total_kib === 0) {
      return html`
        <article class="card" part="swap">
          <h3>Swap</h3>
          <p class="detail">Not configured</p>
        </article>
      `
    }

    const used = swapUsedKib(memory)
    const percent = usagePercent(used, memory.swap_total_kib)
    return html`
      <article class="card" part="swap">
        <h3>Swap</h3>
        <dl>
          <div><dt>Total</dt><dd>${formatKib(memory.swap_total_kib)}</dd></div>
          <div><dt>Used</dt><dd>${formatKib(used)}</dd></div>
          <div><dt>Free</dt><dd>${formatKib(memory.swap_free_kib)}</dd></div>
        </dl>
        <div class="bar" aria-label=${`Swap used: ${formatPercent(percent)}`}>
          <span style=${`width: ${percent}%`}></span>
        </div>
        <p class="detail">${formatPercent(percent)} used</p>
      </article>
    `
  }

  private renderLoad() {
    const info = this.info
    if (!info) return nothing

    const loads = [
      ['1 min', info.load.one],
      ['5 min', info.load.five],
      ['15 min', info.load.fifteen],
    ] as const

    return html`
      <article class="card" part="load">
        <h3>Load average</h3>
        <dl>
          ${loads.map(
            ([label, value]) => html`
              <div>
                <dt>${label}</dt>
                <dd>${formatLoad(value)}</dd>
              </div>
            `,
          )}
        </dl>
        <div class="load-bars">
          ${loads.map(([label, value]) => {
            const percent = loadPercent(value, info.logical_cores)
            return html`
              <div
                class="load-row"
                title=${`${label}: ${formatLoad(value)} (${formatPercent(percent)} of ${info.logical_cores} cores)`}
              >
                <span>${label}</span>
                <div class="bar"><span style=${`width: ${percent}%`}></span></div>
              </div>
            `
          })}
        </div>
      </article>
    `
  }

  private renderOverview() {
    const info = this.info
    if (!info) {
      return html`<p class="empty">
        ${this.loading ? 'Loading host info…' : 'No host information yet.'}
      </p>`
    }

    return html`
      <section class="summary" part="summary">
        <div>
          <p class="eyebrow">Host</p>
          <h2>${info.hostname}</h2>
          <p class="os">
            ${info.os.name}
            ${info.os.version ? html`<span>${info.os.version}</span>` : nothing}
          </p>
        </div>
        <dl class="quick">
          <div><dt>Kernel</dt><dd>${info.os.kernel || 'unknown'}</dd></div>
          <div><dt>Architecture</dt><dd>${info.os.arch || 'unknown'}</dd></div>
          <div><dt>Uptime</dt><dd>${formatUptime(info.uptime_seconds)}</dd></div>
        </dl>
      </section>

      <section class="grid" part="details">
        <article class="card" part="cpu">
          <h3>CPU</h3>
          <p class="model">${info.cpu_model || 'Unknown CPU'}</p>
          <p class="detail">${info.logical_cores} logical cores</p>
        </article>

        ${this.renderLoad()} ${this.renderMemory()} ${this.renderSwap()}
      </section>
    `
  }

  override render() {
    return html`
      <section class="host-info" part="host-info" aria-busy=${this.loading}>
        <header class="toolbar">
          <div class="status">
            ${this.renderStatus()}
            <span class="muted">${this.statusDetail()}</span>
          </div>
          <div class="actions">
            <label class="field">
              Interval
              <select
                aria-label="Refresh interval"
                .value=${String(this.refreshInterval)}
                @change=${this.onIntervalChange}
              >
                ${this.intervals().map(
                  (interval) => html`
                    <option value=${String(interval)}>
                      ${interval / 1000}s
                    </option>
                  `,
                )}
              </select>
            </label>
            <button type="button" ?disabled=${this.loading} @click=${this.refresh}>
              Refresh
            </button>
            <button type="button" @click=${this.toggleWatch}>
              ${this.watching ? 'Pause' : 'Resume'}
            </button>
          </div>
        </header>

        ${this.errorMessage
          ? html`<div class="error" role="alert">${this.errorMessage}</div>`
          : nothing}
        ${this.renderOverview()}
      </section>
    `
  }

  static styles = css`
    :host {
      display: block;
      color: var(--text, #dbe6f2);
      font: 14px/1.45 var(--sans, system-ui, sans-serif);
    }

    .host-info {
      display: flex;
      flex-direction: column;
      gap: 12px;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 10px;
      background: var(--panel, #101721);
      padding: 12px;
    }

    .toolbar {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      justify-content: space-between;
      gap: 10px;
    }

    .status,
    .actions {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: 8px;
    }

    .status-pill {
      display: inline-flex;
      align-items: center;
      gap: 6px;
      padding: 2px 8px;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 999px;
      color: var(--muted, #8b9bad);
      font: 12px/1.4 var(--mono, monospace);
    }

    .status-pill::before {
      width: 7px;
      height: 7px;
      content: '';
      border-radius: 50%;
      background: var(--muted, #8b9bad);
    }

    .status-pill[data-status='connecting']::before {
      background: #fbbf24;
    }

    .status-pill[data-status='live']::before {
      background: #34d399;
    }

    .status-pill[data-status='error']::before {
      background: #f87171;
    }

    .muted,
    .detail,
    .empty {
      color: var(--muted, #8b9bad);
    }

    button,
    select {
      border: 1px solid var(--panel-border, #223041);
      border-radius: 6px;
      background: #172231;
      color: var(--text, #dbe6f2);
      padding: 4px 9px;
      font: inherit;
    }

    button {
      cursor: pointer;
    }

    button:disabled {
      opacity: 0.45;
      cursor: default;
    }

    button:hover:not(:disabled) {
      border-color: var(--accent, #7dd3fc);
    }

    button:focus-visible,
    select:focus-visible {
      outline: 2px solid var(--accent, #7dd3fc);
      outline-offset: 2px;
    }

    .field {
      display: inline-flex;
      align-items: center;
      gap: 6px;
      color: var(--muted, #8b9bad);
      font: 12px/1.4 var(--mono, monospace);
    }

    .error {
      padding: 8px 10px;
      border-radius: 6px;
      background: rgba(248, 113, 113, 0.12);
      color: #fecaca;
    }

    .summary {
      display: flex;
      flex-wrap: wrap;
      align-items: end;
      justify-content: space-between;
      gap: 16px;
      padding: 12px;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 8px;
      background: #0c131c;
    }

    h2 {
      margin: 2px 0 4px;
      font: 600 clamp(24px, 4vw, 34px) / 1.1 var(--mono, monospace);
      letter-spacing: -0.04em;
    }

    h3 {
      margin: 0 0 8px;
      color: var(--accent, #7dd3fc);
      font: 600 11px/1.4 var(--mono, monospace);
      letter-spacing: 0.08em;
      text-transform: uppercase;
    }

    .eyebrow {
      margin: 0;
      color: var(--muted, #8b9bad);
      font: 600 10px/1.4 var(--mono, monospace);
      letter-spacing: 0.1em;
      text-transform: uppercase;
    }

    .os {
      margin: 0;
      color: var(--muted, #8b9bad);
    }

    .os span {
      margin-left: 4px;
    }

    .quick,
    .card dl {
      display: grid;
      gap: 4px;
      margin: 0;
    }

    .quick {
      grid-template-columns: repeat(3, minmax(0, auto));
      gap: 8px 18px;
    }

    .quick div,
    .card dl div {
      display: flex;
      gap: 8px;
      justify-content: space-between;
    }

    dt {
      color: var(--muted, #8b9bad);
      font: 12px/1.4 var(--mono, monospace);
    }

    dd {
      margin: 0;
      font: 12px/1.4 var(--mono, monospace);
    }

    .grid {
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
      gap: 10px;
    }

    .card {
      min-width: 0;
      padding: 10px 12px;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 8px;
      background: #0c131c;
    }

    .model {
      margin: 0 0 4px;
      overflow-wrap: anywhere;
    }

    .detail {
      margin: 6px 0 0;
      font: 12px/1.4 var(--mono, monospace);
    }

    .bar {
      height: 6px;
      margin-top: 8px;
      overflow: hidden;
      border-radius: 999px;
      background: #1b2735;
    }

    .bar span {
      display: block;
      height: 100%;
      border-radius: inherit;
      background: linear-gradient(90deg, #38bdf8, #34d399);
    }

    .load-bars {
      display: grid;
      gap: 5px;
      margin-top: 8px;
    }

    .load-row {
      display: grid;
      grid-template-columns: 44px minmax(0, 1fr);
      align-items: center;
      gap: 8px;
      color: var(--muted, #8b9bad);
      font: 11px/1.4 var(--mono, monospace);
    }

    .load-row .bar {
      margin: 0;
    }

    .empty {
      margin: 0;
      padding: 24px 12px;
      text-align: center;
      border: 1px dashed var(--panel-border, #223041);
      border-radius: 8px;
    }

    @media (max-width: 700px) {
      .quick {
        grid-template-columns: 1fr;
      }
    }
  `
}

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

declare global {
  interface HTMLElementTagNameMap {
    'anth-host-info': AnthHostInfo
  }
}
