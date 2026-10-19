import { LitElement, css, html } from 'lit'
import { customElement, property } from 'lit/decorators.js'
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
  type ProcessTreeRow,
} from '../lib/processes/display'

/**
 * Tree-grid process view. `rows` arrive already ordered, with a precomputed
 * connector prefix per depth. The parent owns sorting and selection.
 *
 * @fires anth-process-select - `{ pid }`
 * @fires anth-process-sort - `{ key }`
 */
@customElement('anth-process-tree')
export class AnthProcessTree extends LitElement {
  @property({ attribute: false })
  rows: ProcessTreeRow[] = []

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
      <div class="tree-scroll" part="scroll">
        <div class="grid head" role="row">
          <div class="cell tree-head" role="columnheader">TREE</div>
          ${PROCESS_COLUMNS.map((column) => this.renderHeader(column))}
        </div>

        <div class="tree" role="tree" aria-label="Process tree">
          ${this.rows.length === 0
            ? html`<div class="empty">${this.emptyMessage}</div>`
            : this.rows.map((row) => this.renderRow(row))}
        </div>
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
      <div
        class=${`cell head-cell ${column.numeric ? 'number' : ''}`}
        role="columnheader"
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
      </div>
    `
  }

  private renderRow(row: ProcessTreeRow) {
    const { process, depth, prefix } = row
    const selected = process.pid === this.selectedPid

    return html`
      <div
        class=${`grid row ${selected ? 'selected' : ''}`}
        role="treeitem"
        aria-level=${depth + 1}
        aria-selected=${selected}
        tabindex="0"
        @click=${() => this.select(process.pid)}
        @keydown=${(event: KeyboardEvent) => this.onRowKeydown(event, process.pid)}
      >
        <span class="cell tree-prefix" aria-hidden="true">${prefix}</span>
        <span class="cell mono">${process.pid}</span>
        <span class="cell mono muted">${process.ppid}</span>
        <span class="cell">${process.user}</span>
        <span class="cell number mono">
          ${process.cpu_percent === null
            ? '—'
            : `${process.cpu_percent.toFixed(1)}%`}
        </span>
        <span class="cell number mono">${formatKib(process.rss_kib)}</span>
        <span class="cell">
          <span class="state" data-state=${process.state}>
            ${stateLabel(process.state)}
          </span>
        </span>
        <span class="cell command" title=${process.command}>
          ${process.command}
        </span>
      </div>
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

    .tree-scroll {
      max-height: var(--process-table-height, 32rem);
      overflow: auto;
      scrollbar-color: var(--process-border, #263241) transparent;
      scrollbar-width: thin;
    }

    .grid {
      display: grid;
      grid-template-columns:
        minmax(4.5rem, max-content)
        4.5rem
        4.5rem
        5.5rem
        5.5rem
        6.5rem
        6.5rem
        minmax(10rem, 1fr);
      align-items: center;
      min-width: 62rem;
    }

    .head {
      position: sticky;
      top: 0;
      z-index: 1;
      color: var(--process-muted, #7f90a3);
      font-size: 0.78em;
      font-weight: 600;
      letter-spacing: 0.06em;
      text-transform: uppercase;
      background: var(--process-head-bg, #131a23);
      border-bottom: 1px solid var(--process-border, #263241);
    }

    .tree-head {
      padding: 0.4rem 0.6rem;
    }

    .head-cell {
      padding: 0;
    }

    .head-cell.number .sort {
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

    [aria-sort='ascending'] .sort-mark,
    [aria-sort='descending'] .sort-mark {
      opacity: 1;
    }

    .row {
      line-height: 1.25;
      border-bottom: 1px solid
        color-mix(in srgb, var(--process-border, #263241) 55%, transparent);
      outline: none;
      cursor: pointer;
    }

    .row:hover,
    .row:focus-visible {
      background: rgba(125, 211, 252, 0.08);
    }

    .row.selected {
      background: rgba(125, 211, 252, 0.14);
      box-shadow: inset 3px 0 0 var(--process-accent, #7dd3fc);
    }

    .tree .row:last-child {
      border-bottom: 0;
    }

    .cell {
      min-width: 0;
      padding: 0.22rem 0.6rem;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    .tree-prefix {
      color: var(--process-muted, #7f90a3);
      font-size: 0.94em;
      white-space: pre;
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

    .command {
      max-width: 44rem;
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

    .empty {
      padding: 1.25rem 0.8rem;
      color: var(--process-muted, #7f90a3);
      text-align: center;
    }
  `
}

declare global {
  interface HTMLElementTagNameMap {
    'anth-process-tree': AnthProcessTree
  }
}
