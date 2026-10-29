/**
 * `anth-host-info`: the host snapshot, as the panel presents it.
 *
 * A summary strip with the machine's identity, then one card per resource, each
 * with a bar for the number that is easier to read as a proportion than as
 * bytes.
 *
 * The widget owns its refresh: it subscribes to `/api/host-info/watch` and
 * repaints only its body, so a live card costs the rest of the shell nothing.
 * Two controls sit in its toolbar — the interval and a pause — because a panel
 * that refreshes every second should let the person reading it decide whether
 * it does.
 */

import { LitElement, css, html, nothing } from 'lit'
import { customElement, property, state } from 'lit/decorators.js'
import { localized, msg } from '@lit/localize'

import type { HostInfo, HostInfoClient } from 'anth-wire'
import { describeError } from 'anth-wire'
import './icon'
import {
  formatKib,
  formatLoad,
  formatPercent,
  formatUptime,
  loadPercent,
  usagePercent,
} from 'anth-wire'

/** The intervals the selector offers, in milliseconds. */
const INTERVALS = [1000, 2000, 5000, 10000, 30000] as const

type FeedStatus = 'idle' | 'connecting' | 'live' | 'paused' | 'stopped' | 'error'

@customElement('anth-host-info')
@localized()
export class AnthHostInfo extends LitElement {
  @property({ attribute: false })
  client?: HostInfoClient

  @state()
  private info?: HostInfo

  @state()
  private status: FeedStatus = 'idle'

  @state()
  private error = ''

  @state()
  private loading = false

  @state()
  private interval: number = INTERVALS[1]

  @state()
  private paused = false

  private unsubscribe?: () => void

  override connectedCallback(): void {
    super.connectedCallback()
    this.start()
  }

  /**
   * Closing here is what keeps a paused or removed card from holding a
   * connection open. The tab panel removes a hidden tab, so this is also what
   * stops the stream when the operator switches away.
   */
  override disconnectedCallback(): void {
    super.disconnectedCallback()
    this.stop()
  }

  /** Re-reads the snapshot now; the panel's FAB calls it. */
  async refresh(): Promise<void> {
    if (!this.client) {
      return
    }
    this.loading = true
    try {
      this.info = await this.client.get()
      this.error = ''
      if (!this.paused && (this.status === 'live' || this.status === 'connecting')) {
        this.status = 'live'
      }
    } catch (error) {
      this.error = describeError(error)
      this.status = 'error'
    } finally {
      this.loading = false
    }
  }

  private start(): void {
    this.stop()
    if (!this.client) {
      this.status = 'idle'
      return
    }
    this.paused = false
    this.status = 'connecting'
    this.error = ''
    this.unsubscribe = this.client.watch(this.interval, {
      onSnapshot: (info) => {
        this.info = info
        // A frame clears the previous refusal: the data on screen is current.
        this.error = ''
        this.loading = false
        this.status = 'live'
      },
      onError: (error) => {
        this.error = error.message
        this.status = 'error'
      },
      onStatus: (status) => {
        if (status === 'connecting') {
          this.status = 'connecting'
        } else if (status === 'live') {
          this.status = 'live'
        } else if (this.status !== 'error') {
          this.status = 'stopped'
        }
      },
    })
  }

  private stop(): void {
    this.unsubscribe?.()
    this.unsubscribe = undefined
  }

  private pause(): void {
    this.stop()
    this.paused = true
    this.status = 'paused'
  }

  private resume(): void {
    this.start()
  }

  /** Pause/resume when live; retry when the stream refused. */
  private toggleFeed(): void {
    if (this.paused || this.status === 'error' || this.status === 'stopped') {
      this.resume()
      return
    }
    this.pause()
  }

  private changeInterval(event: Event): void {
    this.interval = Number((event.currentTarget as HTMLSelectElement).value)
    if (!this.paused) {
      // SSE carries the schedule in the query, so a new period is a new URL.
      this.start()
    }
  }

  private statusText(): string {
    switch (this.status) {
      case 'live':
        return msg('live')
      case 'connecting':
        return msg('connecting')
      case 'error':
        return msg('error')
      case 'paused':
        return msg('paused')
      case 'stopped':
        return msg('stopped')
      case 'idle':
        return msg('idle')
    }
  }

  /** What the one button does next. */
  private buttonLabel(): string {
    if (this.paused) {
      return msg('Resume')
    }
    if (this.status === 'error' || this.status === 'stopped') {
      return msg('Retry')
    }
    return msg('Pause')
  }

  private buttonIcon(): string {
    if (this.paused) {
      return 'play'
    }
    if (this.status === 'error' || this.status === 'stopped') {
      return 'refresh'
    }
    return 'pause'
  }

  private pair(label: string, value: string) {
    return html`<div><dt>${label}</dt><dd>${value}</dd></div>`
  }

  private bar(label: string, percent: number) {
    return html`<div class="bar" role="img" aria-label=${label} title=${label}>
      <span style=${`width: ${percent}%`}></span>
    </div>`
  }

  private renderToolbar() {
    return html`
      <div class="toolbar" part="toolbar">
        <span class="status" data-state=${this.status} role="status">${this.statusText()}</span>
        <label class="field">
          <span class="field-label">${msg('Refresh')}</span>
          <select
            aria-label=${msg('Refresh interval')}
            .value=${String(this.interval)}
            @change=${(event: Event) => this.changeInterval(event)}
          >
            ${INTERVALS.map(
              (ms) =>
                html`<option value=${String(ms)} ?selected=${ms === this.interval}>
                  ${msg('every')} ${intervalLabel(ms)}
                </option>`,
            )}
          </select>
        </label>
        <button
          type="button"
          aria-label=${this.buttonLabel()}
          title=${this.buttonLabel()}
          aria-pressed=${this.paused ? 'true' : 'false'}
          ?disabled=${this.loading}
          @click=${() => this.toggleFeed()}
        >
          <anth-icon name=${this.buttonIcon()}></anth-icon>
          <span class="btn-label">${this.buttonLabel()}</span>
        </button>
      </div>
    `
  }

  private renderSummary(info: HostInfo) {
    return html`
      <section class="summary" part="summary">
        <div class="who">
          <p class="eyebrow">${msg('Host')}</p>
          <h2>${info.hostname || msg('unknown')}</h2>
          <p class="os">
            ${info.os.name || msg('unknown')}
            ${info.os.version ? html`<span>· ${info.os.version}</span>` : nothing}
          </p>
        </div>
        <dl class="quick">
          ${this.pair(msg('Kernel'), info.os.kernel || msg('unknown'))}
          ${this.pair(msg('Architecture'), info.os.arch || msg('unknown'))}
          ${this.pair(msg('Uptime'), formatUptime(info.uptime_seconds))}
        </dl>
      </section>
    `
  }

  private renderCpu(info: HostInfo) {
    return html`
      <article class="card" part="cpu">
        <h3>${msg('CPU')}</h3>
        <p class="model">${info.cpu_model || msg('Unknown CPU')}</p>
        <p class="detail">${info.logical_cores} ${msg('logical cores')}</p>
      </article>
    `
  }

  private renderLoad(info: HostInfo) {
    const loads: ReadonlyArray<readonly [string, number]> = [
      [msg('1 min'), info.load.one],
      [msg('5 min'), info.load.five],
      [msg('15 min'), info.load.fifteen],
    ]
    return html`
      <article class="card" part="load">
        <h3>${msg('Load average')}</h3>
        <dl>${loads.map(([label, value]) => this.pair(label, formatLoad(value)))}</dl>
        <div class="bars">
          ${loads.map(([label, value]) => {
            const percent = loadPercent(value, info.logical_cores)
            return html`<div class="bar-row">
              <span class="bar-label">${label}</span>
              ${this.bar(
                `${label}: ${formatLoad(value)} · ${formatPercent(percent)}`,
                percent,
              )}
            </div>`
          })}
        </div>
      </article>
    `
  }

  private renderMemory(info: HostInfo) {
    const used = Math.max(0, info.memory.total_kib - info.memory.available_kib)
    const percent = usagePercent(used, info.memory.total_kib)
    return html`
      <article class="card" part="memory">
        <h3>${msg('Memory')}</h3>
        <dl>
          ${this.pair(msg('Total'), formatKib(info.memory.total_kib))}
          ${this.pair(msg('Used'), formatKib(used))}
          ${this.pair(msg('Available'), formatKib(info.memory.available_kib))}
        </dl>
        ${this.bar(msg('Memory used'), percent)}
        <p class="detail">${formatPercent(percent)} ${msg('used')}</p>
      </article>
    `
  }

  private renderSwap(info: HostInfo) {
    if (info.memory.swap_total_kib === 0) {
      return html`
        <article class="card" part="swap">
          <h3>${msg('Swap')}</h3>
          <p class="detail">${msg('Not configured')}</p>
        </article>
      `
    }
    const used = Math.max(0, info.memory.swap_total_kib - info.memory.swap_free_kib)
    const percent = usagePercent(used, info.memory.swap_total_kib)
    return html`
      <article class="card" part="swap">
        <h3>${msg('Swap')}</h3>
        <dl>
          ${this.pair(msg('Total'), formatKib(info.memory.swap_total_kib))}
          ${this.pair(msg('Used'), formatKib(used))}
          ${this.pair(msg('Free'), formatKib(info.memory.swap_free_kib))}
        </dl>
        ${this.bar(msg('Swap used'), percent)}
        <p class="detail">${formatPercent(percent)} ${msg('used')}</p>
      </article>
    `
  }

  private renderBody() {
    if (this.error) {
      return html`<p class="error" role="alert">${this.error}</p>`
    }
    const info = this.info
    if (!info) {
      if (this.paused) {
        return html`<p class="empty">${msg('Paused — press Resume to read the host again.')}</p>`
      }
      if (this.loading || this.client) {
        return html`<p class="empty">${msg('Reading the host…')}</p>`
      }
      return html`<p class="empty">${msg('No host information yet.')}</p>`
    }
    return html`
      ${this.renderSummary(info)}
      <div class="grid" part="details">
        ${this.renderCpu(info)} ${this.renderLoad(info)} ${this.renderMemory(info)}
        ${this.renderSwap(info)}
      </div>
    `
  }

  override render() {
    return html`
      ${this.client ? this.renderToolbar() : nothing}
      <div class="body" part="body">${this.renderBody()}</div>
    `
  }

  static styles = css`
    :host {
      display: block;
      color: var(--text, #dbe6f2);
      font: 14px/1.45 var(--sans, system-ui, sans-serif);
    }

    .body { display: flex; flex-direction: column; gap: 0.75rem; }

    .toolbar {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: 0.5rem 0.75rem;
      margin-bottom: 0.75rem;
      padding: 0.5rem 0.65rem;
      border-radius: 8px;
      border: 1px solid var(--panel-border, #223041);
      background: var(--panel, #101721);
    }
    .toolbar .status {
      display: inline-flex;
      align-items: center;
      gap: 0.35rem;
      color: var(--muted, #8b9bad);
      font: 600 10px/1.4 var(--mono, monospace);
      letter-spacing: 0.08em;
      text-transform: uppercase;
    }
    .toolbar .status::before {
      content: '';
      width: 6px;
      height: 6px;
      border-radius: 999px;
      background: currentColor;
    }
    .toolbar .status[data-state='live'] { color: var(--ok, #34d399); }
    .toolbar .status[data-state='error'] { color: var(--err, #f87171); }
    .toolbar .status[data-state='connecting'] { color: var(--accent, #7dd3fc); }

    .field {
      display: inline-flex;
      align-items: center;
      gap: 0.35rem;
      color: var(--muted, #8b9bad);
      font: 12px/1.4 var(--mono, monospace);
    }
    .toolbar select,
    .toolbar button {
      min-height: 32px;
      padding: 2px 8px;
      border-radius: 6px;
      color: var(--text, #dbe6f2);
      border: 1px solid var(--panel-border, #223041);
      background: var(--widget-bg, rgba(16, 23, 33, 0.5));
      font: 12px/1.4 var(--mono, monospace);
    }
    .toolbar button { display: inline-flex; align-items: center; gap: 0.4rem;
                      cursor: pointer; margin-left: auto; }
    .toolbar anth-icon { font-size: 14px; }
    .toolbar button:hover { border-color: var(--accent, #7dd3fc); }
    .toolbar select:focus-visible,
    .toolbar button:focus-visible {
      outline: 2px solid var(--accent, #7dd3fc);
      outline-offset: 1px;
    }

    .summary {
      display: flex;
      flex-wrap: wrap;
      align-items: end;
      justify-content: space-between;
      gap: 0.9rem 1rem;
      padding: 0.85rem 1rem;
      border-radius: 8px;
      border: 1px solid var(--panel-border, #223041);
      background: var(--panel, #101721);
    }
    .who { min-width: 0; }
    h2 {
      margin: 0.15rem 0 0.2rem;
      overflow-wrap: anywhere;
      font: 600 clamp(1.35rem, 3.4vw, 2rem) / 1.1 var(--mono, monospace);
      letter-spacing: -0.03em;
    }
    .eyebrow {
      margin: 0;
      color: var(--muted, #8b9bad);
      font: 600 10px/1.4 var(--mono, monospace);
      letter-spacing: 0.1em;
      text-transform: uppercase;
    }
    .os { margin: 0; color: var(--muted, #8b9bad); font-size: 13px; }
    .os span { margin-left: 0.25rem; }

    .grid {
      display: grid;
      gap: 0.65rem;
      grid-template-columns: repeat(auto-fit, minmax(min(190px, 100%), 1fr));
    }
    .card {
      min-width: 0;
      padding: 0.75rem 0.9rem;
      border-radius: 8px;
      border: 1px solid var(--panel-border, #223041);
      background: var(--widget-bg, rgba(16, 23, 33, 0.5));
    }
    h3 {
      margin: 0 0 0.5rem;
      color: var(--accent, #7dd3fc);
      font: 600 11px/1.4 var(--mono, monospace);
      letter-spacing: 0.08em;
      text-transform: uppercase;
    }

    dl { display: grid; gap: 0.2rem; margin: 0; }
    dl > div {
      display: flex;
      flex-wrap: wrap;
      gap: 0.2rem 0.5rem;
      justify-content: space-between;
      align-items: baseline;
    }
    .quick { gap: 0.35rem 1rem; grid-template-columns: repeat(auto-fit, minmax(8.5rem, auto)); }
    .quick > div { justify-content: start; gap: 0.45rem; }
    dt { color: var(--muted, #8b9bad); font: 12px/1.4 var(--mono, monospace); }
    dd {
      margin: 0;
      overflow-wrap: anywhere;
      font: 12px/1.4 var(--mono, monospace);
      font-variant-numeric: tabular-nums;
    }

    .model { margin: 0 0 0.25rem; overflow-wrap: anywhere; }
    .detail,
    .empty { color: var(--muted, #8b9bad); font: 12px/1.4 var(--mono, monospace); }
    .detail { margin: 0.35rem 0 0; }

    .bars { display: grid; gap: 0.3rem; margin-top: 0.5rem; }
    .bar-row {
      display: grid;
      grid-template-columns: 2.75rem minmax(0, 1fr);
      align-items: center;
      gap: 0.5rem;
    }
    .bar {
      display: block;
      height: 6px;
      overflow: hidden;
      border-radius: 999px;
      background: rgba(139, 155, 173, 0.22);
    }
    .bar > span {
      display: block;
      height: 100%;
      border-radius: inherit;
      background: linear-gradient(90deg, var(--accent, #7dd3fc), var(--ok, #34d399));
    }
    .bar-label { color: var(--muted, #8b9bad); font: 11px/1.4 var(--mono, monospace); }

    .empty {
      margin: 0;
      padding: 1.5rem 0.75rem;
      text-align: center;
      border: 1px dashed var(--panel-border, #223041);
      border-radius: 8px;
    }
    .error {
      margin: 0;
      padding: 0.6rem 0.75rem;
      border-radius: 8px;
      border: 1px solid rgba(248, 113, 113, 0.4);
      background: rgba(248, 113, 113, 0.12);
      color: var(--err, #f87171);
    }

    @media (max-width: 48rem) {
      .toolbar .btn-label { display: none; }
      .toolbar button:has(.btn-label) {
        width: 44px;
        height: 44px;
        padding: 0;
        justify-content: center;
      }
    }
    @media (pointer: coarse) {
      .toolbar select,
      .toolbar button { min-height: 44px; font-size: max(16px, 1rem); }
    }

    @media (max-width: 30rem) {
      .toolbar button { margin-left: 0; }
      .summary { align-items: start; padding: 0.75rem; }
      .quick { grid-template-columns: minmax(0, 1fr); }
      .quick > div { justify-content: space-between; }
      .grid { grid-template-columns: minmax(0, 1fr); }
      dd,
      .detail { font-size: 13px; }
      .bar { height: 8px; }
    }
  `
}

function intervalLabel(ms: number): string {
  return ms % 1000 === 0 ? `${ms / 1000}s` : `${ms}ms`
}
