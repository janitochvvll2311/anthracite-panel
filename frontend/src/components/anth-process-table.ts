import { LitElement, css, html, nothing } from 'lit'
import { customElement, property } from 'lit/decorators.js'
import type { ProcessRow } from '../lib/processes/process-client'

export const ANTH_PROCESS_SELECT_EVENT = 'anth-process-select'

export interface AnthProcessSelectDetail {
  pid: number
}

const STATE_LABELS: Record<string, string> = {
  R: 'running',
  S: 'sleeping',
  D: 'disk',
  Z: 'zombie',
  T: 'stopped',
  t: 'tracing',
  I: 'idle',
  X: 'dead',
}

/**
 * Presentational process table.
 *
 * It receives rows already filtered and sorted by its owner and emits the
 * selected pid. It never fetches or signals anything itself.
 *
 * @fires anth-process-select - `{ pid }` when a row is selected.
 */
@customElement('anth-process-table')
export class AnthProcessTable extends LitElement {
  @property({ attribute: false })
  processes: ProcessRow[] = []

  @property({ type: Number, attribute: 'selected-pid' })
  selectedPid?: number

  /** Truncation or command-length notice from the daemon. */
  @property({ type: String })
  warning = ''

  @property({ type: String, attribute: 'empty-message' })
  emptyMessage = 'No processes.'

  override render() {
    return html`
      <section class="table-shell" part="table">
        ${this.warning === ''
          ? nothing
          : html`
              <p class="warning" part="warning" role="status">
                ${this.warning}
              </p>
            `}
        <div class="table-wrap" part="scroll">
          <table>
            <thead>
              <tr>
                <th scope="col">PID</th>
                <th scope="col">PPID</th>
                <th scope="col">USER</th>
                <th scope="col" class="number">CPU</th>
                <th scope="col" class="number">MEM</th>
                <th scope="col">STATE</th>
                <th scope="col">COMMAND</th>
              </tr>
            </thead>
            <tbody>
              ${this.processes.length === 0
                ? html`
                    <tr>
                      <td class="empty" colspan="7">${this.emptyMessage}</td>
                    </tr>
                  `
                : this.processes.map((process) => this.renderRow(process))}
            </tbody>
          </table>
        </div>
      </section>
    `
  }

  private renderRow(process: ProcessRow) {
    const selected = process.pid === this.selectedPid
    return html`
      <tr
        class=${selected ? 'selected' : ''}
        tabindex="0"
        role="button"
        aria-selected=${selected}
        @click=${() => this.select(process.pid)}
        @keydown=${(event: KeyboardEvent) => this.onRowKeydown(event, process.pid)}
      >
        <td class="mono">${process.pid}</td>
        <td class="mono muted">${process.ppid}</td>
        <td>${process.user}</td>
        <td class="number mono">
          ${process.cpu_percent === null
            ? '—'
            : `${process.cpu_percent.toFixed(1)}%`}
        </td>
        <td class="number mono">${formatKib(process.rss_kib)}</td>
        <td>
          <span class="state" data-state=${process.state}>
            ${STATE_LABELS[process.state] ?? process.state}
          </span>
        </td>
        <td class="command" title=${process.command}>${process.command}</td>
      </tr>
    `
  }

  private select(pid: number): void {
    this.selectedPid = pid
    this.dispatchEvent(
      new CustomEvent<AnthProcessSelectDetail>(ANTH_PROCESS_SELECT_EVENT, {
        detail: { pid },
        bubbles: true,
        composed: true,
      }),
    )
  }

  private onRowKeydown(event: KeyboardEvent, pid: number): void {
    if (event.key !== 'Enter' && event.key !== ' ') return
    event.preventDefault()
    this.select(pid)
  }

  static styles = css`
    :host {
      display: block;
      min-height: 0;
      color: var(--process-fg, #d7e2ee);
      font: 13px/1.45 var(
          --process-font,
          ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace
        );
    }

    * {
      box-sizing: border-box;
    }

    .table-shell {
      overflow: hidden;
      background: var(--process-bg, #0b0f14);
      border: 1px solid var(--process-border, #263241);
      border-radius: var(--process-radius, 10px);
    }

    .warning {
      margin: 0;
      padding: 0.55rem 0.8rem;
      color: #fde68a;
      font-size: 0.92em;
      border-bottom: 1px solid var(--process-border, #263241);
      background: rgba(251, 191, 36, 0.08);
    }

    .table-wrap {
      max-height: var(--process-table-height, 32rem);
      overflow: auto;
      scrollbar-color: var(--process-border, #263241) transparent;
      scrollbar-width: thin;
    }

    table {
      width: 100%;
      border-collapse: collapse;
      white-space: nowrap;
    }

    thead {
      position: sticky;
      top: 0;
      z-index: 1;
      background: var(--process-head-bg, #131a23);
    }

    th {
      padding: 0.55rem 0.7rem;
      color: var(--process-muted, #7f90a3);
      font-size: 0.78em;
      font-weight: 600;
      letter-spacing: 0.06em;
      text-align: left;
      text-transform: uppercase;
      border-bottom: 1px solid var(--process-border, #263241);
    }

    td {
      padding: 0.42rem 0.7rem;
      border-bottom: 1px solid
        color-mix(in srgb, var(--process-border, #263241) 55%, transparent);
    }

    tbody tr {
      outline: none;
      cursor: pointer;
    }

    tbody tr:hover,
    tbody tr:focus-visible {
      background: rgba(125, 211, 252, 0.08);
    }

    tbody tr.selected {
      background: rgba(125, 211, 252, 0.14);
      box-shadow: inset 3px 0 0 var(--process-accent, #7dd3fc);
    }

    tbody tr:last-child td {
      border-bottom: 0;
    }

    .number {
      text-align: right;
    }

    .mono {
      font-variant-numeric: tabular-nums;
    }

    .muted {
      color: var(--process-muted, #7f90a3);
    }

    .empty {
      padding: 1.5rem 0.8rem;
      color: var(--process-muted, #7f90a3);
      text-align: center;
    }

    .command {
      max-width: 44rem;
      overflow: hidden;
      text-overflow: ellipsis;
    }

    .state {
      display: inline-block;
      min-width: 4.8rem;
      padding: 0.05rem 0.4rem;
      color: var(--process-muted, #7f90a3);
      font-size: 0.85em;
      text-align: center;
      border: 1px solid var(--process-border, #263241);
      border-radius: 999px;
    }

    .state[data-state='R'] {
      color: #86efac;
      border-color: rgba(134, 239, 172, 0.35);
    }

    .state[data-state='Z'] {
      color: #fca5a5;
      border-color: rgba(252, 165, 165, 0.35);
    }
  `
}

function formatKib(kib: number): string {
  const mib = kib / 1024
  if (mib >= 1024) return `${(mib / 1024).toFixed(1)} GiB`
  if (mib >= 1) return `${mib.toFixed(1)} MiB`
  return `${Math.round(kib)} KiB`
}

declare global {
  interface HTMLElementTagNameMap {
    'anth-process-table': AnthProcessTable
  }

  interface HTMLElementEventMap {
    'anth-process-select': CustomEvent<AnthProcessSelectDetail>
  }
}
