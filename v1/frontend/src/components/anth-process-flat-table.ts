import { LitElement, css, html } from 'lit'
import { customElement, property } from 'lit/decorators.js'
import type { ProcessRow } from '../lib/processes/process-client'
import {
  ANTH_PROCESS_SELECT_EVENT,
  ANTH_PROCESS_SORT_EVENT,
  PROCESS_COLUMNS,
  formatKib,
  stateLabel,
  type AnthProcessSelectDetail,
  type AnthProcessSortDetail,
  type ProcessSortDirection,
  type ProcessSortKey,
} from '../lib/processes/display'

/**
 * Compact flat process table. The parent sorts `rows` and owns selection.
 *
 * @fires anth-process-select - `{ pid }`
 * @fires anth-process-sort - `{ key }`
 */
@customElement('anth-process-flat-table')
export class AnthProcessFlatTable extends LitElement {
  @property({ attribute: false })
  rows: ProcessRow[] = []

  @property({ type: Number, attribute: 'selected-pid' })
  selectedPid?: number

  @property({ type: String, attribute: 'sort-key' })
  sortKey: ProcessSortKey = 'cpu'

  @property({ type: String, attribute: 'sort-direction' })
  sortDirection: ProcessSortDirection = 'desc'

  @property({ type: String, attribute: 'empty-message' })
  emptyMessage = 'No processes.'

  override render() {
    return html`
      <div class="table-wrap" part="scroll">
        <table>
          <thead>
            <tr>
              ${PROCESS_COLUMNS.map((column) => this.renderHeader(column))}
            </tr>
          </thead>
          <tbody>
            ${this.rows.length === 0
              ? html`
                  <tr>
                    <td class="empty" colspan="7">${this.emptyMessage}</td>
                  </tr>
                `
              : this.rows.map((process) => this.renderRow(process))}
          </tbody>
        </table>
      </div>
    `
  }

  private renderHeader(column: (typeof PROCESS_COLUMNS)[number]) {
    const active = this.sortKey === column.key
    const direction = active ? this.sortDirection : undefined
    const ariaSort =
      direction === 'asc'
        ? 'ascending'
        : direction === 'desc'
          ? 'descending'
          : 'none'

    return html`
      <th
        scope="col"
        class=${column.numeric ? 'number' : ''}
        aria-sort=${ariaSort}
      >
        <button
          type="button"
          class="sort"
          @click=${() => this.sort(column.key)}
        >
          ${column.label}
          <span class="sort-mark" aria-hidden="true">
            ${direction === 'asc' ? '↑' : direction === 'desc' ? '↓' : '↕'}
          </span>
        </button>
      </th>
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
        <td class="command" title=${process.command}>${process.command}</td>
        <td class="number mono">${process.threads}</td>
        <td>${process.user}</td>
        <td>
          <span class="state" data-state=${process.state}>
            ${stateLabel(process.state)}
          </span>
        </td>
        <td class="number mono">${formatKib(process.rss_kib)}</td>
        <td class="number mono">
          ${process.cpu_percent === null
            ? '—'
            : `${process.cpu_percent.toFixed(1)}%`}
        </td>
      </tr>
    `
  }

  private sort(key: ProcessSortKey): void {
    this.dispatchEvent(
      new CustomEvent<AnthProcessSortDetail>(ANTH_PROCESS_SORT_EVENT, {
        detail: { key },
        bubbles: true,
        composed: true,
      }),
    )
  }

  private select(pid: number): void {
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
    }

    * {
      box-sizing: border-box;
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
      line-height: 1.25;
    }

    thead {
      position: sticky;
      top: 0;
      z-index: 1;
      background: var(--process-head-bg, #131a23);
    }

    th {
      padding: 0;
      color: var(--process-muted, #7f90a3);
      font-size: 0.78em;
      font-weight: 600;
      letter-spacing: 0.06em;
      text-align: left;
      text-transform: uppercase;
      vertical-align: middle;
      border-bottom: 1px solid var(--process-border, #263241);
    }

    th.number .sort {
      justify-content: flex-end;
    }

    .sort {
      display: flex;
      gap: 0.35rem;
      align-items: center;
      width: 100%;
      padding: 0.4rem 0.6rem;
      color: inherit;
      font: inherit;
      letter-spacing: inherit;
      text-align: inherit;
      text-transform: inherit;
      background: transparent;
      border: 0;
      cursor: pointer;
    }

    .sort:hover {
      color: var(--process-fg, #d7e2ee);
      background: rgba(125, 211, 252, 0.06);
    }

    .sort:focus-visible {
      outline: 2px solid var(--process-accent, #7dd3fc);
      outline-offset: -2px;
    }

    .sort-mark {
      color: var(--process-accent, #7dd3fc);
      font-size: 0.9em;
      opacity: 0.45;
    }

    th[aria-sort='ascending'] .sort-mark,
    th[aria-sort='descending'] .sort-mark {
      opacity: 1;
    }

    td {
      padding: 0.24rem 0.6rem;
      line-height: 1.25;
      vertical-align: middle;
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

    .empty {
      padding: 1.25rem 0.8rem;
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
      padding: 0 0.35rem;
      color: var(--process-muted, #7f90a3);
      font-size: 0.82em;
      line-height: 1.45;
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

declare global {
  interface HTMLElementTagNameMap {
    'anth-process-flat-table': AnthProcessFlatTable
  }
}
