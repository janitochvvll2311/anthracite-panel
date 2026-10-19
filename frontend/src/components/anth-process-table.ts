import { LitElement, css, html, nothing } from 'lit'
import type { PropertyValues } from 'lit'
import { customElement, property, state } from 'lit/decorators.js'
import './anth-process-flat-table'
import './anth-process-tree'
import type { ProcessRow } from '../lib/processes/process-client'
import {
  ANTH_PROCESS_SELECT_EVENT,
  compareProcesses,
  defaultDirection,
  type AnthProcessSelectDetail,
  type AnthProcessSortDetail,
  type ProcessSortDirection,
  type ProcessSortKey,
  type ProcessTreeNode,
  type ProcessView,
} from '../lib/processes/display'

export { ANTH_PROCESS_SELECT_EVENT }
export type { AnthProcessSelectDetail, ProcessView }

/**
 * Process table controller.
 *
 * It owns sort state and chooses between two independent renderers:
 * `anth-process-flat-table` and `anth-process-tree`. Both receive rows that
 * are already sorted, so header clicks behave the same in either view. In tree
 * mode siblings are sorted and each row carries a connector prefix.
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

  protected override updated(changed: PropertyValues<this>): void {
    if (
      changed.has('view') &&
      this.view === 'tree' &&
      (this.sortKey === 'pid' || this.sortKey === 'command')
    ) {
      this.sortKey = 'cpu'
      this.sortDirection = 'desc'
    }
  }

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
        ${this.view === 'tree'
          ? html`
              <anth-process-tree
                .nodes=${this.treeNodes()}
                .selectedPid=${this.selectedPid}
                .sortKey=${this.sortKey}
                .sortDirection=${this.sortDirection}
                .emptyMessage=${this.emptyMessage}
                @anth-process-select=${this.onSelect}
                @anth-process-sort=${this.onSort}
              ></anth-process-tree>
            `
          : html`
              <anth-process-flat-table
                .rows=${this.flatRows()}
                .selectedPid=${this.selectedPid}
                .sortKey=${this.sortKey}
                .sortDirection=${this.sortDirection}
                .emptyMessage=${this.emptyMessage}
                @anth-process-select=${this.onSelect}
                @anth-process-sort=${this.onSort}
              ></anth-process-flat-table>
            `}
      </section>
    `
  }

  private flatRows(): ProcessRow[] {
    return [...this.processes].sort((left, right) =>
      compareProcesses(left, right, this.sortKey, this.sortDirection),
    )
  }

  private treeNodes(): ProcessTreeNode[] {
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

    const visited = new Set<number>()

    const build = (process: ProcessRow, path: Set<number>): ProcessTreeNode => {
      visited.add(process.pid)
      path.add(process.pid)

      const descendants = (children.get(process.pid) ?? [])
        .filter((child) => !path.has(child.pid))
        .sort((left, right) =>
          compareProcesses(left, right, this.sortKey, this.sortDirection),
        )

      return {
        process,
        children: descendants.map((child) => build(child, new Set(path))),
      }
    }

    const nodes = roots
      .sort((left, right) =>
        compareProcesses(left, right, this.sortKey, this.sortDirection),
      )
      .map((root) => build(root, new Set()))

    // Malformed parent cycles have no natural root; show them at depth zero.
    for (const process of this.processes) {
      if (!visited.has(process.pid)) {
        nodes.push(build(process, new Set()))
      }
    }

    return nodes
  }

  private onSort(event: CustomEvent<AnthProcessSortDetail>): void {
    const key = event.detail.key
    if (this.sortKey === key) {
      this.sortDirection = this.sortDirection === 'asc' ? 'desc' : 'asc'
    } else {
      this.sortKey = key
      this.sortDirection = defaultDirection(key)
    }
  }

  private onSelect(event: CustomEvent<AnthProcessSelectDetail>): void {
    this.selectedPid = event.detail.pid
    // The original event keeps bubbling; the monitor also listens for it.
  }

  static styles = css`
    :host {
      display: block;
      min-height: 0;
      color: var(--process-fg, #d7e2ee);
      font: 13px/1.35 var(
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
  `
}

declare global {
  interface HTMLElementTagNameMap {
    'anth-process-table': AnthProcessTable
  }

  interface HTMLElementEventMap {
    'anth-process-select': CustomEvent<AnthProcessSelectDetail>
  }
}
