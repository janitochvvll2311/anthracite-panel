import { LitElement, css, html, nothing } from 'lit'
import { customElement, property, state } from 'lit/decorators.js'
import { localized, msg } from '@lit/localize'

import type { HostInfo, HostInfoClient } from './host-info-client'
import {
  formatKib,
  formatLoad,
  formatPercent,
  formatUptime,
  usagePercent,
} from './format'

@customElement('anth-host-info')
@localized()
export class AnthHostInfo extends LitElement {
  @property({ attribute: false })
  client?: HostInfoClient

  @state()
  private info?: HostInfo

  @state()
  private status: 'idle' | 'connecting' | 'live' | 'error' | 'closed' = 'idle'

  @state()
  private error = ''

  @state()
  private loading = false

  private unsubscribe?: () => void

  override connectedCallback(): void {
    super.connectedCallback()
    this.start()
  }

  override disconnectedCallback(): void {
    super.disconnectedCallback()
    this.stop()
  }

  private start(): void {
    this.stop()
    if (!this.client) {
      this.status = 'idle'
      return
    }
    this.status = 'connecting'
    this.error = ''
    this.unsubscribe = this.client.watch(2000, {
      onSnapshot: (info) => {
        this.info = info
        this.error = ''
      },
      onError: (error) => {
        this.error = error.message
        this.status = 'error'
      },
      onStatus: (status) => {
        this.status = status
      },
    })
  }

  private stop(): void {
    this.unsubscribe?.()
    this.unsubscribe = undefined
    if (this.status !== 'idle') this.status = 'closed'
  }

  private async refresh(): Promise<void> {
    if (!this.client) return
    this.loading = true
    try {
      this.info = await this.client.get()
      this.error = ''
      this.status = 'live'
    } catch (error) {
      this.error = error instanceof Error ? error.message : String(error)
      this.status = 'error'
    } finally {
      this.loading = false
    }
  }

  private renderStatus() {
    const label =
      this.status === 'live'
        ? msg('Live')
        : this.status === 'connecting'
          ? msg('Connecting')
          : this.status === 'error'
            ? msg('Error')
            : msg('Paused')
    return html`<span class="status" data-status=${this.status}>${label}</span>`
  }

  private renderMemory() {
    const memory = this.info?.memory
    if (!memory) return nothing
    const used = Math.max(0, memory.total_kib - memory.available_kib)
    const percent = usagePercent(used, memory.total_kib)
    return html`
      <article class="card">
        <h3>${msg('Memory')}</h3>
        <p class="value">
          ${formatKib(used)} / ${formatKib(memory.total_kib)}
          <span class="muted">(${formatPercent(percent)})</span>
        </p>
        <p class="muted">${formatKib(memory.available_kib)} ${msg('available')}</p>
      </article>
    `
  }

  private renderSwap() {
    const memory = this.info?.memory
    if (!memory) return nothing
    if (memory.swap_total_kib === 0) {
      return html`
        <article class="card">
          <h3>${msg('Swap')}</h3>
          <p class="muted">${msg('Not configured')}</p>
        </article>
      `
    }
    const used = Math.max(0, memory.swap_total_kib - memory.swap_free_kib)
    return html`
      <article class="card">
        <h3>${msg('Swap')}</h3>
        <p class="value">
          ${formatKib(used)} / ${formatKib(memory.swap_total_kib)}
          <span class="muted">(${formatPercent(usagePercent(used, memory.swap_total_kib))})</span>
        </p>
        <p class="muted">${formatKib(memory.swap_free_kib)} ${msg('free')}</p>
      </article>
    `
  }

  private renderOverview() {
    if (!this.info) {
      return html`<p class="muted">${this.loading ? msg('Loading host info…') : msg('No host information yet.')}</p>`
    }
    const info = this.info
    return html`
      <section class="summary">
        <div>
          <p class="eyebrow">${msg('Host')}</p>
          <h2>${info.hostname || msg('unknown')}</h2>
          <p class="muted">
            ${info.os.name || msg('unknown')}
            ${info.os.version ? html`· ${info.os.version}` : nothing}
          </p>
        </div>
        <dl class="quick">
          <div><dt>${msg('Kernel')}</dt><dd>${info.os.kernel || msg('unknown')}</dd></div>
          <div><dt>${msg('Architecture')}</dt><dd>${info.os.arch || msg('unknown')}</dd></div>
          <div><dt>${msg('Uptime')}</dt><dd>${formatUptime(info.uptime_seconds)}</dd></div>
        </dl>
      </section>

      <section class="grid">
        <article class="card">
          <h3>${msg('CPU')}</h3>
          <p class="value">${info.cpu_model || msg('Unknown CPU')}</p>
          <p class="muted">${info.logical_cores} ${msg('logical cores')}</p>
        </article>

        <article class="card">
          <h3>${msg('Load average')}</h3>
          <p class="value">${formatLoad(info.load.one)} ${formatLoad(info.load.five)} ${formatLoad(info.load.fifteen)}</p>
          <p class="muted">${msg('1 / 5 / 15 minutes')}</p>
        </article>

        ${this.renderMemory()} ${this.renderSwap()}
      </section>
    `
  }

  override render() {
    return html`
      <section class="host" aria-busy=${this.loading}>
        <header class="toolbar">
          ${this.renderStatus()}
          <button type="button" ?disabled=${this.loading} @click=${this.refresh}>
            ${msg('Refresh')}
          </button>
        </header>
        ${this.error ? html`<div class="error" role="alert">${this.error}</div>` : nothing}
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
    .host {
      display: flex;
      flex-direction: column;
      gap: 12px;
      padding: 12px;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 10px;
      background: var(--panel, #101721);
    }
    .toolbar {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 10px;
    }
    .status {
      padding: 2px 8px;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 999px;
      color: var(--muted, #8b9bad);
      font: 12px/1.4 var(--mono, monospace);
    }
    .status[data-status='live'] { color: #34d399; }
    .status[data-status='connecting'] { color: #fbbf24; }
    .status[data-status='error'] { color: #f87171; }
    button {
      border: 1px solid var(--panel-border, #223041);
      border-radius: 6px;
      background: #172231;
      color: var(--text, #dbe6f2);
      padding: 4px 9px;
      font: inherit;
      cursor: pointer;
    }
    button:disabled { opacity: .45; cursor: default; }
    button:focus-visible { outline: 2px solid var(--accent, #7dd3fc); outline-offset: 2px; }
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
    h2 { margin: 2px 0 4px; font: 600 clamp(24px, 4vw, 34px) / 1.1 var(--mono, monospace); }
    h3 { margin: 0 0 6px; color: var(--muted, #8b9bad); font: 600 12px/1.4 var(--mono, monospace); letter-spacing: .06em; text-transform: uppercase; }
    .eyebrow { margin: 0; color: var(--muted, #8b9bad); font: 12px/1.4 var(--mono, monospace); text-transform: uppercase; letter-spacing: .08em; }
    .quick { display: flex; flex-wrap: wrap; gap: 18px; margin: 0; }
    .quick div { display: grid; gap: 2px; }
    dt { color: var(--muted, #8b9bad); font: 11px/1.4 var(--mono, monospace); text-transform: uppercase; }
    dd { margin: 0; font: 13px/1.4 var(--mono, monospace); }
    .grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(190px, 1fr)); gap: 10px; }
    .card { padding: 12px; border: 1px solid var(--panel-border, #223041); border-radius: 8px; background: #0c131c; }
    .value { margin: 0 0 4px; font: 14px/1.4 var(--mono, monospace); }
    .muted { margin: 0; color: var(--muted, #8b9bad); }
    .error { padding: 8px 10px; border-radius: 6px; background: rgba(248, 113, 113, .12); color: #fecaca; }
  `
}
