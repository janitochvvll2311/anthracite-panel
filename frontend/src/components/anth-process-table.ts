import { LitElement, css, html, nothing } from 'lit'
import { customElement, property, state } from 'lit/decorators.js'
import type { ProcessRow } from '../lib/processes/process-client'

export const ANTH_PROCESS_SELECT_EVENT = 'anth-process-select'

export interface AnthProcessSelectDetail {
  pid: number
}

export type ProcessSortKey =
  | 'pid'
  | 'ppid'
  | 'user'
  | 'cpu'
  | 'memory'
  | 'state'
  | 'command'

export type ProcessSortDirection = 'asc' | 'desc'
export type ProcessView = 'flat' | 'tree'

interface DisplayRow {
  process: ProcessRow
  depth: number
}

interface Column {
  key: ProcessSortKey
  label: string
  numeric: boolean
}

const COLUMNS: Column[] = [
  { key: 'pid', label: 'PID', numeric: true },
  { key: 'ppid', label: 'PPID', numeric: true },
  { key: 'user', label: 'USER', numeric: false },
  { key: 'cpu', label: 'CPU', numeric: true },
  { key: 'memory', label: 'MEM', numeric: true },
  { key: 'state', label: 'STATE', numeric: false },
  { key: 'command', label: 'COMMAND', numeric: false },
]

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
 * Process table with sortable headers and a flat or tree view.
 *
 * It receives rows already filtered by its owner. Sorting and tree layout are
 * local to the table so a header click works the same in both views. In tree
 * mode, siblings are sorted and children are indented under their parent.
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

  /** `flat` lists every row; `tree` nests by ppid. */
  @property({ type: String })
  view: ProcessView = 'flat'

  @state()
  private sortKey: ProcessSortKey = 'cpu'

  @state()
  private sortDirection: ProcessSortDirection = 'desc'

  override render() {
    const rows = this.displayRows()

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
                ${COLUMNS.map((column) => this.renderHeader(column))}
              </tr>
            </thead>
            <tbody>
              ${rows.length === 0
                ? html`
                    <tr>
                      <td class="empty" colspan="7">${this.emptyMessage}</td>
                    </tr>
                  `
                : rows.map((row) => this.renderRow(row))}
            </tbody>
          </table>
        </div>
      </section>
    `
  }

  private renderHeader(column: Column) {
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
          @click=${() => this.sortBy(column.key)}
        >
          ${column.label}
          <span class="sort-mark" aria-hidden="true">
            ${direction === 'asc' ? '↑' : direction === 'desc' ? '↓' : '↕'}
          </span>
        </button>
      </th>
    `
  }

  private renderRow({ process, depth }: DisplayRow) {
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
        <td class="command" title=${process.command}>
          <span class="tree-mark" style=${`margin-left: ${depth * 1.15}rem`}>
            ${depth === 0 ? '' : '└─ '}
          </span>
          ${process.command}
        </td>
      </tr>
    `
  }

  private sortBy(key: ProcessSortKey): void {
    if (this.sortKey === key) {
      this.sortDirection = this.sortDirection === 'asc' ? 'desc' : 'asc'
      return
    }

    this.sortKey = key
    this.sortDirection = defaultDirection(key)
  }

  private displayRows(): DisplayRow[] {
    if (this.view === 'flat') {
      return [...this.processes]
        .sort((left, right) => this.compareRows(left, right))
        .map((process) => ({ process, depth: 0 }))
    }

    return this.treeRows()
  }

  private treeRows(): DisplayRow[] {
    const byPid = new Map(this.processes.map((process) => [process.pid, process]))
    const children = new Map<number, ProcessRow[]>()
    const roots: ProcessRow[] = []

    for (const process of this.processes) {
      if (process.ppid === process.pid || !byPid.has(process.ppid)) {
        roots.push(process)
        continue
      }

      const siblings = children.get(process.ppid) ?? []
      siblings.push(process)
      children.set(process.ppid, siblings)
    }

    const rows: DisplayRow[] = []
    const visited = new Set<number>()

    const walk = (process: ProcessRow, depth: number) => {
      if (visited.has(process.pid)) return
      visited.add(process.pid)
      rows.push({ process, depth })

      const siblings = children.get(process.pid) ?? []
      siblings
        .sort((left, right) => this.compareRows(left, right))
        .forEach((child) => walk(child, depth + 1))
    }

    roots
      .sort((left, right) => this.compareRows(left, right))
      .forEach((root) => walk(root, 0))

    // A malformed parent cycle has no natural root; show the rest at depth 0.
    for (const process of this.processes) {
      if (!visited.has(process.pid)) walk(process, 0)
    }

    return rows
  }

  private compareRows(left: ProcessRow, right: ProcessRow): number {
    const a = sortValue(left, this.sortKey)
    const b = sortValue(right, this.sortKey)
    const direction = this.sortDirection === 'asc' ? 1 : -1

    if (a === null && b === null) return left.pid - right.pid
    if (a === null) return 1
    if (b === null) return -1
    if (typeof a === 'string' && typeof b === 'string') {
      return a.localeCompare(b) * direction
    }
    return (Number(a) - Number(b)) * direction
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
      padding: 0;
      color: var(--process-muted, #7f90a3);
      font-size: 0.78em;
      font-weight: 600;
      letter-spacing: 0.06em;
      text-align: left;
      text-transform: uppercase;
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
      padding: 0.55rem 0.7rem;
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

    .tree-mark {
      color: var(--process-muted, #7f90a3);
      white-space: pre;
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

function defaultDirection(key: ProcessSortKey): ProcessSortDirection {
  return key === 'cpu' || key === 'memory' ? 'desc' : 'asc'
}

function sortValue(
  process: ProcessRow,
  key: ProcessSortKey,
): number | string | null {
  switch (key) {
    case 'cpu':
      return process.cpu_percent
    case 'memory':
      return process.rss_kib
    case 'pid':
      return process.pid
    case 'ppid':
      return process.ppid
    case 'user':
      return process.user
    case 'state':
      return process.state
    case 'command':
      return process.command === '' ? process.name : process.command
  }
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
