import { LitElement, css, html, nothing } from 'lit'
import { customElement, property, state } from 'lit/decorators.js'
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
  type ProcessTreeNode,
} from '../lib/processes/display'

interface TreeDisplayRow {
  process: ProcessRow
  depth: number
  prefix: string
  hasChildren: boolean
  expanded: boolean
}

/**
 * Free-form tree process view.
 *
 * `nodes` arrive already sorted by the parent. The tree component owns the
 * collapsed set, computes connector prefixes and renders rows without wrapping
 * them in a table. Branching nodes get an expand/collapse control.
 *
 * @fires anth-process-select - `{ pid }`
 * @fires anth-process-sort - `{ key }`
 */
@customElement('anth-process-tree')
export class AnthProcessTree extends LitElement {
  @property({ attribute: false })
  nodes: ProcessTreeNode[] = []

  @property({ type: Number, attribute: 'selected-pid' })
  selectedPid?: number

  @property({ type: String, attribute: 'sort-key' })
  sortKey: ProcessSortKey = 'cpu'

  @property({ type: String, attribute: 'sort-direction' })
  sortDirection: ProcessSortDirection = 'desc'

  @property({ type: String, attribute: 'empty-message' })
  emptyMessage = 'No processes.'

  @state()
  private collapsed = new Set<number>()

  override render() {
    const rows = this.visibleRows()

    return html`
      <div class="tree-scroll" part="scroll">
        <div class="grid head" role="row">
          <div class="cell tree-head" role="columnheader">TREE</div>
          ${PROCESS_COLUMNS.map((column) => this.renderHeader(column))}
        </div>

        <div class="tree" role="tree" aria-label="Process tree">
          ${rows.length === 0
            ? html`<div class="empty">${this.emptyMessage}</div>`
            : rows.map((row) => this.renderRow(row))}
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

  private renderRow(row: TreeDisplayRow) {
    const { process, depth, prefix, hasChildren, expanded } = row
    const selected = process.pid === this.selectedPid

    return html`
      <div
        class=${`grid row ${selected ? 'selected' : ''}`}
        role="treeitem"
        aria-level=${depth + 1}
        aria-selected=${selected}
        aria-expanded=${hasChildren ? expanded : nothing}
        tabindex="0"
        @click=${() => this.select(process.pid)}
        @keydown=${(event: KeyboardEvent) => this.onRowKeydown(event, process.pid)}
      >
        <span class="cell tree-cell">
          <span class="tree-prefix" aria-hidden="true">${prefix}</span>
          ${hasChildren
            ? html`
                <button
                  type="button"
                  class="toggle"
                  aria-expanded=${expanded}
                  aria-label=${`${expanded ? 'Collapse' : 'Expand'} process ${process.pid}`}
                  @click=${(event: MouseEvent) => this.toggle(process.pid, event)}
                >
                  ${expanded ? '▾' : '▸'}
                </button>
              `
            : html`<span class="toggle-spacer" aria-hidden="true"></span>`}
        </span>
        <span class="cell mono">${process.pid}</span>
        <span class="cell command" title=${process.command}>
          ${process.command}
        </span>
        <span class="cell">${process.user}</span>
        <span class="cell">
          <span class="state" data-state=${process.state}>
            ${stateLabel(process.state)}
          </span>
        </span>
        <span class="cell number mono">${formatKib(process.rss_kib)}</span>
        <span class="cell number mono">
          ${process.cpu_percent === null
            ? '—'
            : `${process.cpu_percent.toFixed(1)}%`}
        </span>
      </div>
    `
  }

  private visibleRows(): TreeDisplayRow[] {
    const rows: TreeDisplayRow[] = []

    const walk = (
      node: ProcessTreeNode,
      depth: number,
      ancestorsLast: boolean[],
      isLast: boolean,
    ) => {
      const expanded = !this.collapsed.has(node.process.pid)
      const prefix =
        depth === 0
          ? ''
          : ancestorsLast.map((last) => (last ? '   ' : '│  ')).join('') +
            (isLast ? '└─ ' : '├─ ')

      rows.push({
        process: node.process,
        depth,
        prefix,
        hasChildren: node.children.length > 0,
        expanded,
      })

      if (!expanded || node.children.length === 0) return

      node.children.forEach((child, index) => {
        walk(
          child,
          depth + 1,
          [...ancestorsLast, isLast],
          index === node.children.length - 1,
        )
      })
    }

    this.nodes.forEach((node, index) => {
      walk(node, 0, [], index === this.nodes.length - 1)
    })

    return rows
  }

  private toggle(pid: number, event: MouseEvent): void {
    event.stopPropagation()
    const collapsed = new Set(this.collapsed)
    if (collapsed.has(pid)) {
      collapsed.delete(pid)
    } else {
      collapsed.add(pid)
    }
    this.collapsed = collapsed
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
        minmax(4rem, max-content)
        4.5rem
        minmax(14rem, 1fr)
        8rem
        7rem
        7rem
        6rem;
      align-items: center;
      min-width: 52rem;
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
      line-height: 1.3;
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
      padding: 0.2rem 0.5rem;
      white-space: nowrap;
    }

    .tree-cell {
      display: flex;
      gap: 0.15rem;
      align-items: center;
      padding-right: 0.15rem;
    }

    .tree-prefix {
      color: var(--process-muted, #7f90a3);
      font-size: 0.94em;
      white-space: pre;
    }

    .toggle {
      display: inline-grid;
      flex: 0 0 auto;
      place-items: center;
      width: 1.15rem;
      height: 1.15rem;
      padding: 0;
      color: var(--process-accent, #7dd3fc);
      font: inherit;
      font-size: 0.9em;
      line-height: 1;
      background: transparent;
      border: 0;
      border-radius: 3px;
      cursor: pointer;
    }

    .toggle:hover {
      background: rgba(125, 211, 252, 0.14);
    }

    .toggle:focus-visible {
      outline: 2px solid var(--process-accent, #7dd3fc);
      outline-offset: 1px;
    }

    .toggle-spacer {
      flex: 0 0 auto;
      width: 1.15rem;
    }

    .command {
      overflow: visible;
      overflow-wrap: anywhere;
      white-space: normal;
    }

    .number {
      text-align: right;
    }

    .mono {
      font-variant-numeric: tabular-nums;
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
