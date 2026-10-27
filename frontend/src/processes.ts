/**
 * `anth-processes`: the host process table, live.
 *
 * The component owns its feed: it subscribes to `/api/processes/watch` and
 * repaints on every snapshot. CPU percentages are computed here from two
 * consecutive snapshots, because the daemon never guesses an interval.
 *
 * Two views share one sorted row set: a flat table and a tree nested by ppid.
 * Signals and spawns are separate switches. The action bar only appears when
 * `[process] control` allows signals, and the start form only when
 * `[process] spawn` allows starting programs.
 */

import { LitElement, css, html, nothing } from 'lit'
import { customElement, property, state } from 'lit/decorators.js'
import { localized, msg, str } from '@lit/localize'

import './icon'
import { describeError } from './errors'
import { formatKib, formatPercent } from './format'
import {
  buildTree,
  compareProcesses,
  defaultDirection,
  parseCommandLine,
  processColumns,
  stateLabel,
  visibleTreeRows,
} from './lib/processes/display'
import type {
  ProcessColumn,
  ProcessRow,
  ProcessSortDirection,
  ProcessSortKey,
  ProcessView,
  TreeDisplayRow,
} from './lib/processes/display'
import { PROCESS_SIGNALS } from './processes-client'
import type {
  ProcessInfo,
  ProcessSignal,
  ProcessSnapshot,
  ProcessesClient,
} from './processes-client'

/** The intervals the selector offers, in milliseconds. */
const INTERVALS = [1000, 2000, 5000, 10000, 30000] as const

type FeedStatus = 'idle' | 'connecting' | 'live' | 'paused' | 'stopped' | 'error'

@customElement('anth-processes')
@localized()
export class AnthProcesses extends LitElement {
  @property({ attribute: false })
  client?: ProcessesClient

  @state()
  private snapshot?: ProcessSnapshot

  @state()
  private rows: ProcessRow[] = []

  @state()
  private status: FeedStatus = 'idle'

  @state()
  private error = ''

  @state()
  private notice = ''

  @state()
  private interval: number = INTERVALS[1]

  @state()
  private paused = false

  @state()
  private query = ''

  @state()
  private view: ProcessView = 'flat'

  /** Phones stack the metrics under each row instead of beside it. */
  @state()
  private compact = false

  @state()
  private sort: ProcessSortKey = 'cpu'

  @state()
  private descending = true

  @state()
  private collapsed = new Set<number>()

  @state()
  private selectedKey?: string

  @state()
  private signal: ProcessSignal = 'term'

  @state()
  private group = false

  @state()
  private busy = false

  @state()
  private spawnOpen = false

  @state()
  private spawnCommand = ''

  @state()
  private spawnCwd = ''

  @state()
  private spawnError = ''

  private unsubscribe?: () => void
  private resizeObserver?: ResizeObserver
  /** Last measured width of the metric pane; 480px is 30rem at the default. */
  private metricsWidth = 480
  private samples = new Map<string, number>()
  private lastTimestamp = 0

  override connectedCallback(): void {
    super.connectedCallback()
    // A phone starts compact before the first measurement, so it never flashes
    // the two-pane layout. The width rule below overrides this hint.
    this.compact = globalThis.matchMedia?.('(max-width: 48rem)').matches ?? false
    this.observeWidth()
    this.start()
  }

  /** Closing here is what stops the stream when the tab is removed. */
  override disconnectedCallback(): void {
    super.disconnectedCallback()
    this.resizeObserver?.disconnect()
    this.resizeObserver = undefined
    this.stop()
  }

  private observeWidth(): void {
    if (typeof ResizeObserver === 'undefined') {
      return
    }
    this.resizeObserver = new ResizeObserver(() => this.updateLayout())
    this.resizeObserver.observe(this)
  }

  /**
   * The component's own breakpoint: the two-pane layout only fits while the
   * identity side is at least as wide as the metric side. A narrow window or a
   * narrow parent switches to the stacked cards even on a desktop viewport.
   */
  private updateLayout(): void {
    const width = this.getBoundingClientRect().width
    if (width <= 0) {
      return
    }
    const pane = this.renderRoot.querySelector<HTMLElement>('.pane.metrics')
    if (pane) {
      const measured = pane.getBoundingClientRect().width
      if (measured > 0) {
        this.metricsWidth = measured
      }
    }
    this.compact = width - this.metricsWidth < this.metricsWidth
  }

  protected override updated(): void {
    this.updateLayout()
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
      onSnapshot: (snapshot) => this.apply(snapshot),
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

  /** Fetches one page now, without restarting the stream. */
  private async refresh(): Promise<void> {
    if (!this.client || this.busy) {
      return
    }
    try {
      this.apply(await this.client.list())
    } catch (error) {
      this.error = describeError(error)
      this.status = 'error'
    }
  }

  /** Stores a snapshot with its CPU samples computed against the last one. */
  private apply(snapshot: ProcessSnapshot): void {
    const elapsed = (snapshot.timestamp_ms - this.lastTimestamp) / 1000
    const usable = this.lastTimestamp > 0 && elapsed > 0 && snapshot.clock_ticks > 0
    const rows: ProcessRow[] = snapshot.items.map((process) => {
      const previous = this.samples.get(keyOf(process))
      const cpu_percent =
        usable && previous !== undefined && process.cpu_ticks >= previous
          ? ((process.cpu_ticks - previous) / snapshot.clock_ticks / elapsed) * 100
          : null
      return { ...process, cpu_percent }
    })
    this.samples = new Map(
      snapshot.items.map((process) => [keyOf(process), process.cpu_ticks]),
    )
    this.lastTimestamp = snapshot.timestamp_ms
    this.rows = rows
    this.snapshot = snapshot
    this.error = ''
    this.status = 'live'
    if (this.selectedKey && !rows.some((row) => keyOf(row) === this.selectedKey)) {
      this.selectedKey = undefined
    }
  }

  private get selected(): ProcessRow | undefined {
    return this.rows.find((process) => keyOf(process) === this.selectedKey)
  }

  private get total(): number {
    return this.snapshot?.total ?? this.rows.length
  }

  private filtered(): ProcessRow[] {
    const query = this.query.trim().toLowerCase()
    if (query === '') {
      return this.rows
    }
    return this.rows.filter((process) =>
      [
        String(process.pid),
        process.user,
        process.name,
        process.command,
        stateLabel(process.state),
      ].some((value) => value.toLowerCase().includes(query)),
    )
  }

  private sorted(): ProcessRow[] {
    return [...this.filtered()].sort((left, right) =>
      compareProcesses(left, right, this.sort, this.direction()),
    )
  }

  private direction(): ProcessSortDirection {
    return this.descending ? 'desc' : 'asc'
  }

  private sortBy(key: ProcessSortKey): void {
    if (key === 'pid' || key === 'command') {
      // The tree leaves these two unsorted: hierarchy owns their order.
      if (this.view === 'tree') return
    }
    if (this.sort === key) {
      this.descending = !this.descending
      return
    }
    this.sort = key
    this.descending = defaultDirection(key) === 'desc'
  }

  private sortAria(key: ProcessSortKey): 'ascending' | 'descending' | 'none' {
    if (this.sort !== key) {
      return 'none'
    }
    return this.descending ? 'descending' : 'ascending'
  }

  private changeView(view: ProcessView): void {
    this.view = view
    if (view === 'tree' && (this.sort === 'pid' || this.sort === 'command')) {
      this.sort = 'cpu'
      this.descending = true
    }
    void this.updateComplete.then(() => {
      // Both views start at the left edge: the tree shows its roots with their
      // own pid and command next to each node.
      const pane = this.renderRoot.querySelector<HTMLElement>('.pane.identity')
      if (pane) pane.scrollLeft = 0
    })
  }

  private select(process: ProcessInfo): void {
    this.selectedKey = keyOf(process)
  }

  private onRowKey(event: KeyboardEvent, process: ProcessInfo): void {
    if (event.key !== 'Enter' && event.key !== ' ') {
      return
    }
    event.preventDefault()
    this.select(process)
  }

  private toggleCollapse(pid: number, event: MouseEvent): void {
    event.stopPropagation()
    const collapsed = new Set(this.collapsed)
    if (collapsed.has(pid)) {
      collapsed.delete(pid)
    } else {
      collapsed.add(pid)
    }
    this.collapsed = collapsed
  }

  private async sendSignal(): Promise<void> {
    const process = this.selected
    if (!this.client || !process || this.busy) {
      return
    }
    const label = process.command === '' ? process.name : process.command
    if (
      !confirm(
        msg(
          str`Send ${this.signal.toUpperCase()} to PID ${process.pid} (${label})?`,
        ),
      )
    ) {
      return
    }
    this.busy = true
    try {
      await this.client.signal(
        process.pid,
        process.start_ticks,
        this.signal,
        this.group,
      )
      this.error = ''
      const name = this.signal.toUpperCase()
      this.notice = this.group
        ? msg(str`Sent ${name} to the group led by ${process.pid}`)
        : msg(str`Sent ${name} to process ${process.pid}`)
    } catch (error) {
      this.error = describeError(error)
    } finally {
      this.busy = false
    }
  }

  private openSpawn(): void {
    this.spawnOpen = true
    this.spawnCommand = ''
    this.spawnCwd = ''
    this.spawnError = ''
    this.notice = ''
    this.error = ''
  }

  private closeSpawn(): void {
    if (this.busy) return
    this.spawnOpen = false
  }

  /** Replaces the program word and keeps the arguments already typed. */
  private useAllowedProgram(program: string): void {
    const rest = this.spawnCommand.trim().split(/\s+/).slice(1).join(' ')
    this.spawnCommand = rest === '' ? program : `${program} ${rest}`
    this.spawnError = ''
  }

  private async submitSpawn(): Promise<void> {
    if (!this.client || this.busy) {
      return
    }
    const parsed = parseCommandLine(this.spawnCommand.trim())
    if (parsed.program === '') {
      this.spawnError = msg('Enter a program to start.')
      return
    }
    if (!parsed.program.startsWith('/')) {
      this.spawnError = msg(
        'The program must be an absolute path, for example /bin/ls.',
      )
      return
    }
    const cwd = this.spawnCwd.trim()
    if (cwd !== '' && !cwd.startsWith('/')) {
      this.spawnError = msg('The working directory must be an absolute path.')
      return
    }
    const line = [parsed.program, ...parsed.args].join(' ')
    if (!confirm(msg(str`Start ${line}?`))) {
      return
    }
    this.busy = true
    try {
      const started = await this.client.spawn(
        parsed.program,
        parsed.args,
        cwd === '' ? undefined : cwd,
      )
      this.spawnError = ''
      this.notice = msg(str`Started ${started.program} (pid ${started.pid})`)
      this.spawnOpen = false
    } catch (error) {
      this.spawnError = describeError(error)
    } finally {
      this.busy = false
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

  private cpuText(process: ProcessRow): string {
    return process.cpu_percent === null
      ? '—'
      : formatPercent(process.cpu_percent)
  }

  private renderToolbar() {
    const spawn = this.snapshot?.spawn_enabled === true
    return html`
      <div class="toolbar" part="toolbar">
        <span class="status" data-state=${this.status} role="status">
          ${this.statusText()}
        </span>
        <span
          class="count"
          title=${this.total > this.rows.length
            ? msg(str`Only the first ${this.rows.length} processes are loaded.`)
            : nothing}
        >
          ${this.filtered().length} / ${this.total} ${msg('processes')}
        </span>
        <span class="spacer"></span>
        <div class="view-switch" role="group" aria-label=${msg('Process view')}>
          <button
            type="button"
            aria-pressed=${this.view === 'flat' ? 'true' : 'false'}
            @click=${() => this.changeView('flat')}
          >
            ${msg('Flat')}
          </button>
          <button
            type="button"
            aria-pressed=${this.view === 'tree' ? 'true' : 'false'}
            @click=${() => this.changeView('tree')}
          >
            ${msg('Tree')}
          </button>
        </div>
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
        ${spawn
          ? html`<button
              type="button"
              aria-expanded=${this.spawnOpen ? 'true' : 'false'}
              @click=${() => (this.spawnOpen ? this.closeSpawn() : this.openSpawn())}
            >
              <anth-icon name="play"></anth-icon>
              <span class="btn-label">${msg('Start process')}</span>
            </button>`
          : nothing}
        <button type="button" @click=${() => void this.refresh()} ?disabled=${this.busy}>
          <anth-icon name="refresh"></anth-icon>
          <span class="btn-label">${msg('Refresh')}</span>
        </button>
        <button
          type="button"
          aria-label=${this.buttonLabel()}
          title=${this.buttonLabel()}
          aria-pressed=${this.paused ? 'true' : 'false'}
          @click=${() => this.toggleFeed()}
        >
          <anth-icon name=${this.buttonIcon()}></anth-icon>
          <span class="btn-label">${this.buttonLabel()}</span>
        </button>
      </div>
    `
  }

  private renderFilters() {
    const tree = this.view === 'tree'
    const columns = processColumns().filter(
      (column) =>
        !(tree && (column.key === 'pid' || column.key === 'command')),
    )
    return html`
      <div class="filters" part="filters">
        <input
          type="search"
          aria-label=${msg('Filter processes')}
          placeholder=${msg('Filter by pid, user, name or command')}
          .value=${this.query}
          @input=${(event: Event) =>
            (this.query = (event.currentTarget as HTMLInputElement).value)}
        />
        ${this.compact
          ? html`
              <label class="field">
                <span class="field-label">${msg('Sort')}</span>
                <select
                  aria-label=${msg('Sort by')}
                  @change=${(event: Event) => {
                    this.sort = (event.currentTarget as HTMLSelectElement)
                      .value as ProcessSortKey
                    this.descending = defaultDirection(this.sort) === 'desc'
                  }}
                >
                  ${columns.map(
                    (column) =>
                      html`<option
                        value=${column.key}
                        ?selected=${column.key === this.sort}
                      >
                        ${column.label}
                      </option>`,
                  )}
                </select>
              </label>
              <button
                type="button"
                class="direction"
                aria-label=${msg('Toggle sort direction')}
                title=${msg('Toggle sort direction')}
                @click=${() => (this.descending = !this.descending)}
              >
                <anth-icon
                  name=${this.descending ? 'chevron-down' : 'chevron-up'}
                ></anth-icon>
              </button>
            `
          : nothing}
      </div>
    `
  }

  private renderSpawn() {
    const allow = this.snapshot?.spawn_allow ?? []
    return html`
      <form
        class="spawn"
        part="spawn"
        @submit=${(event: Event) => {
          event.preventDefault()
          void this.submitSpawn()
        }}
      >
        <p class="hint">
          ${msg('The daemon only starts programs listed in')}
          <code>[process] spawn_allow</code>.
        </p>
        <label class="field column">
          <span class="field-label">${msg('Command')}</span>
          <input
            type="text"
            name="command"
            autofocus
            placeholder="/bin/ls -la /tmp"
            .value=${this.spawnCommand}
            @input=${(event: Event) => {
              this.spawnCommand = (event.currentTarget as HTMLInputElement).value
              this.spawnError = ''
            }}
          />
        </label>
        <label class="field column">
          <span class="field-label">${msg('Working directory (optional)')}</span>
          <input
            type="text"
            name="cwd"
            placeholder="/home/operator"
            .value=${this.spawnCwd}
            @input=${(event: Event) => {
              this.spawnCwd = (event.currentTarget as HTMLInputElement).value
              this.spawnError = ''
            }}
          />
        </label>
        ${allow.length === 0
          ? html`<p class="hint">
              ${msg('No program is allowed. Add one to `[process] spawn_allow`.')}
            </p>`
          : html`
              <div class="allowlist" part="allowlist">
                <span class="hint">${msg('Allowed programs')}</span>
                <div class="chips">
                  ${allow.map(
                    (program) => html`<button
                      type="button"
                      class="chip"
                      @click=${() => this.useAllowedProgram(program)}
                    >
                      ${program}
                    </button>`,
                  )}
                </div>
              </div>
            `}
        ${this.spawnError
          ? html`<p class="error" role="alert">${this.spawnError}</p>`
          : nothing}
        <div class="spawn-actions">
          <button type="button" @click=${() => this.closeSpawn()} ?disabled=${this.busy}>
            ${msg('Cancel')}
          </button>
          <button type="submit" ?disabled=${this.busy || allow.length === 0}>
            ${this.busy ? msg('Starting…') : msg('Start')}
          </button>
        </div>
      </form>
    `
  }

  private renderActions() {
    const process = this.selected
    const control = this.snapshot?.control_enabled === true
    const selection = process
      ? html`
          <div class="selection-id">
            <span class="pid">PID ${process.pid}</span>
            <span class="muted">${process.user}</span>
            <span class="selection-command" title=${process.command}>
              ${process.command !== '' ? process.command : process.name}
            </span>
          </div>
          <div class="selection-metrics">
            <span class="state" data-state=${process.state}>
              ${stateLabel(process.state)}
            </span>
            <span>${msg(str`${process.threads} threads`)}</span>
            <span>${formatKib(process.rss_kib)}</span>
            <span class="selection-cpu">${this.cpuText(process)} CPU</span>
          </div>
        `
      : html`<span class="muted">${msg('Select a process to send a signal.')}</span>`
    return html`
      <footer class="actions" part="actions">
        <div class="selection">${selection}</div>
        ${control
          ? html`
              <div class="signal">
                <select
                  aria-label=${msg('Signal')}
                  .value=${this.signal}
                  @change=${(event: Event) =>
                    (this.signal = (event.currentTarget as HTMLSelectElement)
                      .value as ProcessSignal)}
                >
                  ${PROCESS_SIGNALS.map(
                    (value) => html`<option value=${value}>
                      ${value.toUpperCase()}
                    </option>`,
                  )}
                </select>
                <label class="check">
                  <input
                    type="checkbox"
                    .checked=${this.group}
                    @change=${(event: Event) =>
                      (this.group = (event.currentTarget as HTMLInputElement).checked)}
                  />
                  ${msg('Group')}
                </label>
                <button
                  type="button"
                  class="danger"
                  ?disabled=${!process || this.busy}
                  @click=${() => void this.sendSignal()}
                >
                  ${this.busy ? msg('Sending…') : msg('Send signal')}
                </button>
              </div>
            `
          : html`<span class="muted control-off">
              ${msg('Control is disabled by the daemon.')}
            </span>`}
      </footer>
    `
  }

  private isSelected(process: ProcessRow): boolean {
    return keyOf(process) === this.selectedKey
  }

  private renderHeadCell(column: ProcessColumn, sortable: boolean) {
    if (!sortable) {
      return html`<div class="cell head-cell" role="columnheader"></div>`
    }
    return html`
      <div
        class=${`cell head-cell col-${column.key} ${column.numeric ? "number" : ""}`}
        role="columnheader"
        aria-sort=${this.sortAria(column.key)}
      >
        <button type="button" class="sort" @click=${() => this.sortBy(column.key)}>
          ${column.label}
          <span class="sort-mark" aria-hidden="true">
            ${this.sort === column.key ? (this.descending ? '↓' : '↑') : '↕'}
          </span>
        </button>
      </div>
    `
  }

  private renderIdentityHead(tree: boolean) {
    if (tree) {
      // A node carries its own pid and command, so there is no shared column
      // to head: one label covers the identity pane.
      return html`
        <div class="row head identity-head" role="row">
          <div class="cell tree-head" role="columnheader">${msg('Tree')}</div>
        </div>
      `
    }
    const columns = processColumns()
    const pid = columns.find((column) => column.key === 'pid')!
    const command = columns.find((column) => column.key === 'command')!
    return html`
      <div class="row head identity-head" role="row">
        ${this.renderHeadCell(pid, true)}
        ${this.renderHeadCell(command, true)}
      </div>
    `
  }

  private renderMetricsHead() {
    return html`
      <div class="row head metrics-head" role="row">
        ${processColumns()
          .filter((column) => column.key !== 'pid' && column.key !== 'command')
          .map((column) => this.renderHeadCell(column, true))}
      </div>
    `
  }

  private renderTreeCell(row: TreeDisplayRow) {
    const { process, depth, ancestorsLast, isLast, hasChildren, expanded } = row
    return html`
      <span class="cell tree-cell">
        <span class="tree-guides" aria-hidden="true">
          ${ancestorsLast.map(
            (last) => html`<span class=${`tree-vline ${last ? 'last' : ''}`}></span>`,
          )}
          ${depth === 0
            ? nothing
            : html`<span class=${`tree-elbow ${isLast ? 'last' : ''}`}></span>`}
        </span>
        ${depth === 0
          ? html`<span class="tree-root-spacer" aria-hidden="true"></span>`
          : nothing}
        ${hasChildren
          ? html`
              <button
                type="button"
                class="toggle"
                aria-expanded=${expanded}
                aria-label=${expanded
                  ? msg(str`Collapse process ${process.pid}`)
                  : msg(str`Expand process ${process.pid}`)}
                @click=${(event: MouseEvent) => this.toggleCollapse(process.pid, event)}
              >
                <svg class="chevron" viewBox="0 0 12 12" aria-hidden="true">
                  <path
                    d="M4 2.5 8 6l-4 3.5"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="1.6"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                  />
                </svg>
              </button>
            `
          : html`<span class="toggle-spacer" aria-hidden="true"></span>`}
      </span>
    `
  }

  /**
   * The identity pane: the tree gutter, the pid and the command.
   *
   * It is the only part of the table with a horizontal scrollbar. The metric
   * pane stays fixed, so a narrow screen or a deep tree never hides CPU and
   * memory behind a table-wide scroll.
   */
  private renderIdentityRow(treeRow: TreeDisplayRow | undefined, process: ProcessRow) {
    const selected = this.isSelected(process)
    return html`
      <div
        class=${`row identity-row ${selected ? 'selected' : ''}`}
        role=${treeRow === undefined ? 'row' : 'treeitem'}
        aria-level=${treeRow === undefined ? nothing : treeRow.depth + 1}
        aria-selected=${selected}
        aria-expanded=${treeRow?.hasChildren ? treeRow.expanded : nothing}
        tabindex="0"
        data-key=${keyOf(process)}
        data-state=${process.state}
        @click=${() => this.select(process)}
        @keydown=${(event: KeyboardEvent) => this.onRowKey(event, process)}
      >
        ${treeRow === undefined ? nothing : this.renderTreeCell(treeRow)}
        <span class="cell pid mono" role="cell">${process.pid}</span>
        <span class="cell command" role="cell" title=${process.command}>
          ${process.command !== '' ? process.command : process.name}
        </span>
      </div>
    `
  }

  private renderMetricsRow(process: ProcessRow) {
    const selected = this.isSelected(process)
    return html`
      <div
        class=${`row metrics-row ${selected ? 'selected' : ''}`}
        role="row"
        aria-selected=${selected}
        data-key=${keyOf(process)}
        @click=${() => this.select(process)}
      >
        <span class="cell col-threads threads number mono" role="cell">
          ${process.threads}
        </span>
        <span class="cell col-user user" role="cell">${process.user}</span>
        <span class="cell col-state state-cell" role="cell">
          <span class="state" data-state=${process.state}>
            ${stateLabel(process.state)}
          </span>
        </span>
        <span class="cell col-memory number mono" role="cell">
          ${formatKib(process.rss_kib)}
        </span>
        <span class="cell col-cpu number mono" role="cell">
          ${this.cpuText(process)}
        </span>
      </div>
    `
  }

  /** Keeps both panes on the same vertical offset without a scroll loop. */
  private readonly syncVertical = (event: Event): void => {
    const source = event.currentTarget as HTMLElement
    const target = this.renderRoot.querySelector<HTMLElement>(
      source.classList.contains('identity') ? '.pane.metrics' : '.pane.identity',
    )
    if (!target || target.scrollTop === source.scrollTop) {
      return
    }
    target.scrollTop = source.scrollTop
  }

  private renderCompactRow(process: ProcessRow, treeRow?: TreeDisplayRow) {
    const selected = this.isSelected(process)
    return html`
      <div
        class=${`compact-row ${treeRow ? 'tree' : ''} ${selected ? 'selected' : ''}`}
        role=${treeRow ? 'treeitem' : 'row'}
        aria-level=${treeRow ? treeRow.depth + 1 : nothing}
        aria-selected=${selected}
        aria-expanded=${treeRow?.hasChildren ? treeRow.expanded : nothing}
        tabindex="0"
        data-key=${keyOf(process)}
        data-state=${process.state}
        @click=${() => this.select(process)}
        @keydown=${(event: KeyboardEvent) => this.onRowKey(event, process)}
      >
        ${treeRow
          ? html`<div class="compact-tree">${this.renderTreeCell(treeRow)}</div>`
          : nothing}
        <div class="compact-identity">
          <span class="pid mono">${process.pid}</span>
          <span class="command" title=${process.command}>
            ${process.command !== '' ? process.command : process.name}
          </span>
        </div>
        <div class="compact-metrics">
          <span class="metric-user">${process.user}</span>
          <span>${msg(str`${process.threads} threads`)}</span>
          <span class="state" data-state=${process.state}>
            ${stateLabel(process.state)}
          </span>
          <span>${formatKib(process.rss_kib)}</span>
          <span class="metric-cpu">${this.cpuText(process)} CPU</span>
        </div>
      </div>
    `
  }

  private renderCompact(
    rows: ProcessRow[],
    treeRows: TreeDisplayRow[],
    tree: boolean,
  ) {
    return html`
      <div class="compact-list" part="table">
        ${rows.map((process, index) =>
          this.renderCompactRow(process, tree ? treeRows[index] : undefined),
        )}
      </div>
    `
  }

  private renderTable() {
    if (!this.snapshot) {
      return html`<p class="empty">${msg('Loading…')}</p>`
    }
    if (this.filtered().length === 0) {
      return html`<p class="empty">${msg('No process matches the filter.')}</p>`
    }

    const tree = this.view === 'tree'
    let treeRows: TreeDisplayRow[] = []
    let rows: ProcessRow[]
    if (tree) {
      treeRows = visibleTreeRows(
        buildTree(this.filtered(), this.sort, this.direction()),
        this.collapsed,
      )
      rows = treeRows.map((row) => row.process)
    } else {
      rows = this.sorted()
    }
    if (rows.length === 0) {
      return html`<p class="empty">${msg('No process matches the filter.')}</p>`
    }
    if (this.compact) {
      return this.renderCompact(rows, treeRows, tree)
    }

    return html`
      <div
        class=${`table-shell ${tree ? 'tree-view' : 'flat-view'}`}
        part="table"
      >
        <div class="pane identity" role="rowgroup" @scroll=${this.syncVertical}>
          ${this.renderIdentityHead(tree)}
          ${tree
            ? treeRows.map((row) => this.renderIdentityRow(row, row.process))
            : rows.map((process) => this.renderIdentityRow(undefined, process))}
        </div>
        <div class="pane metrics" role="rowgroup" @scroll=${this.syncVertical}>
          ${this.renderMetricsHead()}
          ${rows.map((process) => this.renderMetricsRow(process))}
        </div>
      </div>
    `
  }

  private renderNotes() {
    return html`
      ${this.notice ? html`<div class="notice" role="status">${this.notice}</div>` : nothing}
      ${this.error ? html`<div class="error" role="alert">${this.error}</div>` : nothing}
    `
  }

  override render() {
    return html`
      ${this.client ? this.renderToolbar() : nothing}
      ${this.renderNotes()}
      ${this.client ? this.renderFilters() : nothing}
      ${this.spawnOpen ? this.renderSpawn() : nothing}
      ${this.renderTable()}
      ${this.client ? this.renderActions() : nothing}
    `
  }

  static styles = css`
    :host {
      --process-line: rgba(127, 144, 163, 0.45);
      display: block;
      color: var(--text, #dbe6f2);
      font: 13px/1.45 var(--mono, monospace);
    }

    * { box-sizing: border-box; }

    .toolbar,
    .filters,
    .actions,
    .spawn {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: 0.5rem 0.75rem;
      padding: 0.5rem 0.65rem;
      border-radius: 8px;
      border: 1px solid var(--panel-border, #223041);
      background: var(--panel, #101721);
    }
    .toolbar { margin-bottom: 0.6rem; }
    .filters { margin-bottom: 0.6rem; }
    .actions { margin-top: 0.6rem; }
    .spawn { flex-direction: column; align-items: stretch; margin-bottom: 0.6rem; }

    .status {
      display: inline-flex;
      align-items: center;
      gap: 0.35rem;
      color: var(--muted, #8b9bad);
      font: 600 10px/1.4 var(--mono, monospace);
      letter-spacing: 0.08em;
      text-transform: uppercase;
    }
    .status::before {
      content: '';
      width: 6px;
      height: 6px;
      border-radius: 999px;
      background: currentColor;
    }
    .status[data-state='live'] { color: var(--ok, #34d399); }
    .status[data-state='error'] { color: var(--err, #f87171); }
    .status[data-state='connecting'] { color: var(--accent, #7dd3fc); }
    .count { color: var(--muted, #8b9bad); font-size: 11px; }
    .spacer { flex: 1 1 auto; }
    .muted { color: var(--muted, #8b9bad); }

    .view-switch { display: inline-flex; }
    .view-switch button {
      border-radius: 0;
      min-height: 30px;
    }
    .view-switch button:first-child { border-radius: 6px 0 0 6px; }
    .view-switch button:last-child { border-radius: 0 6px 6px 0; border-left: 0; }
    .view-switch button[aria-pressed='true'] {
      border-color: var(--accent, #7dd3fc);
      color: var(--accent, #7dd3fc);
      background: rgba(125, 211, 252, 0.12);
    }

    .field {
      display: inline-flex;
      align-items: center;
      gap: 0.35rem;
      color: var(--muted, #8b9bad);
      font: 12px/1.4 var(--mono, monospace);
    }
    .field.column { flex-direction: column; align-items: stretch; gap: 0.25rem; }
    .field-label { white-space: nowrap; }

    input[type='search'],
    input[type='text'],
    select {
      min-height: 32px;
      padding: 2px 8px;
      border-radius: 6px;
      color: var(--text, #dbe6f2);
      border: 1px solid var(--panel-border, #223041);
      background: var(--widget-bg, rgba(16, 23, 33, 0.5));
      font: 12px/1.4 var(--mono, monospace);
    }
    .filters input { flex: 1 1 12rem; min-width: 0; }
    .filters .field { flex: 0 0 auto; }
    .filters .direction { flex: 0 0 auto; min-width: 36px; }
    .filters .direction anth-icon { font-size: 14px; }
    .field.column input { width: 100%; }

    button {
      display: inline-flex;
      align-items: center;
      justify-content: center;
      gap: 0.4rem;
      min-height: 32px;
      padding: 2px 10px;
      border-radius: 6px;
      cursor: pointer;
      color: var(--text, #dbe6f2);
      border: 1px solid var(--panel-border, #223041);
      background: var(--widget-bg, rgba(16, 23, 33, 0.5));
      font: 12px/1.4 var(--mono, monospace);
    }
    button:hover { border-color: var(--accent, #7dd3fc); }
    button:disabled { opacity: 0.55; cursor: progress; }
    button:focus-visible,
    input:focus-visible,
    select:focus-visible,
    tr:focus-visible,
    .row:focus-visible {
      outline: 2px solid var(--accent, #7dd3fc);
      outline-offset: 1px;
    }
    .toolbar anth-icon { font-size: 14px; }
    .danger { border-color: rgba(248, 113, 113, 0.5); color: #fca5a5; }
    .danger:hover { border-color: var(--err, #f87171); }

    .check {
      display: inline-flex;
      align-items: center;
      gap: 0.35rem;
      color: var(--muted, #8b9bad);
      font: 12px/1.4 var(--mono, monospace);
    }

    .notice,
    .error {
      margin: 0 0 0.6rem;
      padding: 0.5rem 0.7rem;
      border-radius: 8px;
      font-size: 12px;
    }
    .notice {
      color: #a7f3d0;
      border: 1px solid rgba(52, 211, 153, 0.35);
      background: rgba(52, 211, 153, 0.12);
    }
    .error {
      color: #fca5a5;
      border: 1px solid rgba(248, 113, 113, 0.4);
      background: rgba(248, 113, 113, 0.12);
    }
    .hint { margin: 0; color: var(--muted, #8b9bad); font-size: 12px; }
    .hint code { color: var(--accent, #7dd3fc); }

    .allowlist { display: grid; gap: 0.3rem; }
    .chips { display: flex; flex-wrap: wrap; gap: 0.35rem; }
    .chip {
      min-height: 26px;
      padding: 2px 9px;
      border-radius: 999px;
      font-size: 11px;
    }
    .spawn-actions { display: flex; justify-content: flex-end; gap: 0.5rem; }

    .selection {
      display: flex;
      flex: 1 1 20rem;
      flex-direction: column;
      gap: 0.25rem;
      min-width: 0;
      font-size: 12px;
    }
    .selection-id {
      display: flex;
      align-items: baseline;
      gap: 0.6rem;
      min-width: 0;
    }
    .selection .pid { font-weight: 600; }
    .selection-command {
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      color: var(--muted, #8b9bad);
    }
    .selection-metrics {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: 0.25rem 0.8rem;
      color: var(--muted, #8b9bad);
      font-size: 11px;
    }
    .selection-cpu { color: var(--text, #dbe6f2); }
    .signal { display: flex; align-items: center; gap: 0.5rem; }
    .control-off { font-size: 12px; }

    .table-shell {
      display: flex;
      max-height: var(--process-table-height, 34rem);
      border: 1px solid var(--panel-border, #223041);
      border-radius: 8px;
      background: var(--panel, #101721);
      overflow: hidden;
    }
    .pane {
      min-height: 0;
      overflow-y: auto;
      overscroll-behavior: contain;
      scrollbar-color: var(--panel-border, #223041) transparent;
      scrollbar-width: thin;
    }
    /* The identity pane is the only sideways scroll: a deep tree or a long
       command scrolls here, and the metrics never leave the screen. */
    .pane.identity {
      flex: 1 1 auto;
      min-width: 0;
      overflow-x: auto;
      --identity-columns: 4.5rem minmax(10rem, max-content);
    }
    .pane.metrics {
      flex: 0 0 auto;
      overflow-x: hidden;
      border-left: 1px solid var(--panel-border, #223041);
      scrollbar-width: none;
      --metrics-columns: 4.5rem 7rem 6.5rem 6.5rem 5.5rem;
    }
    .pane.metrics::-webkit-scrollbar { display: none; }

    .row {
      display: grid;
      align-items: center;
      height: var(--process-row-height, 27px);
      border-top: 1px solid rgba(34, 48, 65, 0.6);
      cursor: pointer;
    }
    .identity .row { grid-template-columns: var(--identity-columns); }
    .metrics .row { grid-template-columns: var(--metrics-columns); }

    /* In the tree each node carries its own pid and command right after its
       guides, instead of sharing fixed columns with every other row. */
    .table-shell.tree-view .identity-row,
    .table-shell.tree-view .identity-head {
      display: flex;
      gap: 0.35rem;
      width: max-content;
      min-width: 100%;
    }
    .table-shell.tree-view .identity-row .tree-cell,
    .table-shell.tree-view .identity-row .pid,
    .table-shell.tree-view .identity-row .command,
    .table-shell.tree-view .identity-head .tree-head {
      flex: 0 0 auto;
    }
    .table-shell.tree-view .identity-row .pid { padding: 0 0.15rem; }
    .table-shell.tree-view .identity-row .command { padding-right: 1rem; }
    .table-shell.tree-view .identity-head .tree-head { padding: 0 0.5rem; }
    .row.head {
      position: sticky;
      top: 0;
      z-index: 2;
      border-top: 0;
      border-bottom: 1px solid var(--panel-border, #223041);
      background: var(--panel, #101721);
      color: var(--muted, #8b9bad);
      font-size: 10px;
      font-weight: 600;
      letter-spacing: 0.06em;
      text-transform: uppercase;
      cursor: default;
    }
    .row:not(.head):hover { background: rgba(125, 211, 252, 0.06); }
    .row.selected {
      background: rgba(125, 211, 252, 0.14);
      box-shadow: inset 3px 0 0 var(--accent, #7dd3fc);
    }

    .cell {
      display: flex;
      align-items: center;
      min-width: 0;
      height: 100%;
      padding: 0 0.5rem;
      white-space: nowrap;
    }
    .head-cell { padding: 0; }
    .head-cell.number .sort { justify-content: flex-end; }
    .sort {
      display: flex;
      gap: 0.35rem;
      align-items: center;
      width: 100%;
      height: 100%;
      min-height: 0;
      padding: 0 0.5rem;
      color: inherit;
      font: inherit;
      letter-spacing: inherit;
      text-transform: inherit;
      background: transparent;
      border: 0;
      cursor: pointer;
    }
    .sort:hover {
      color: var(--text, #dbe6f2);
      background: rgba(125, 211, 252, 0.06);
    }
    .sort-mark { color: var(--accent, #7dd3fc); opacity: 0.45; }
    [aria-sort='ascending'] .sort-mark,
    [aria-sort='descending'] .sort-mark { opacity: 1; }

    .number { text-align: right; }
    .mono { font-variant-numeric: tabular-nums; }
    .cell.command { padding-right: 1rem; }

    .tree-head { padding: 0 0.5rem; }
    .tree-cell {
      display: flex;
      align-self: stretch;
      align-items: stretch;
      padding: 0 0.15rem 0 0.4rem;
    }
    .tree-guides { display: flex; align-self: stretch; flex: 0 0 auto; }
    .tree-vline,
    .tree-elbow { position: relative; width: 14px; align-self: stretch; }
    .tree-vline:not(.last)::before,
    .tree-elbow::before {
      position: absolute;
      top: 0;
      bottom: 0;
      left: 6px;
      content: '';
      border-left: 1px solid var(--process-line);
    }
    .tree-elbow::after {
      position: absolute;
      top: 50%;
      left: 6px;
      width: 8px;
      content: '';
      border-top: 1px solid var(--process-line);
    }
    .tree-elbow.last::before { bottom: 50%; }
    .toggle {
      display: inline-grid;
      flex: 0 0 auto;
      align-self: center;
      place-items: center;
      width: 1.15rem;
      height: 1.15rem;
      min-height: 0;
      padding: 0;
      color: var(--accent, #7dd3fc);
      background: transparent;
      border: 0;
      border-radius: 3px;
      cursor: pointer;
    }
    .toggle:hover { background: rgba(125, 211, 252, 0.14); }
    .chevron { width: 0.7rem; height: 0.7rem; transition: transform 120ms ease; }
    .toggle[aria-expanded='true'] .chevron { transform: rotate(90deg); }
    .toggle-spacer { flex: 0 0 auto; align-self: center; width: 1.15rem; }
    .tree-root-spacer { flex: 0 0 auto; width: 6px; }

    .state {
      display: inline-block;
      min-width: 4.8rem;
      padding: 0 0.35rem;
      color: var(--muted, #8b9bad);
      font-size: 0.82em;
      line-height: 1.45;
      text-align: center;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 999px;
    }
    .state[data-state='R'] { color: #86efac; border-color: rgba(134, 239, 172, 0.35); }
    .state[data-state='Z'] { color: #fca5a5; border-color: rgba(252, 165, 165, 0.35); }
    .state[data-state='T'] { color: #fbbf24; border-color: rgba(251, 191, 36, 0.35); }

    .compact-list {
      max-height: var(--process-table-height, 34rem);
      overflow-y: auto;
      overscroll-behavior: contain;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 8px;
      background: var(--panel, #101721);
      scrollbar-width: thin;
    }
    .compact-row {
      display: grid;
      grid-template-columns: minmax(0, 1fr);
      gap: 0.15rem 0.45rem;
      /* No vertical padding on the row: the tree guides must reach the card
         edges, or the vertical line breaks between rows. */
      padding: 0;
      border-top: 1px solid rgba(34, 48, 65, 0.6);
      cursor: pointer;
    }
    .compact-row:first-child { border-top: 0; }
    .compact-row.tree { grid-template-columns: auto minmax(0, 1fr); }
    .compact-row:hover { background: rgba(125, 211, 252, 0.06); }
    .compact-row.selected {
      background: rgba(125, 211, 252, 0.14);
      box-shadow: inset 3px 0 0 var(--accent, #7dd3fc);
    }
    .compact-tree {
      grid-row: 1 / span 2;
      display: flex;
      align-self: stretch;
      align-items: stretch;
    }
    .compact-tree .tree-cell { height: 100%; }
    /* The card is two lines tall; the branch points at the identity line, not
       at the middle of the card. */
    .compact-tree { --tree-anchor: 1.05rem; }
    .compact-tree .tree-elbow::after { top: var(--tree-anchor); }
    .compact-tree .tree-elbow.last::before { bottom: calc(100% - var(--tree-anchor)); }
    .compact-tree .toggle,
    .compact-tree .toggle-spacer {
      align-self: flex-start;
      margin-top: calc(var(--tree-anchor) - 0.575rem);
    }
    .compact-row.tree .compact-identity,
    .compact-row.tree .compact-metrics { grid-column: 2; }
    .compact-identity {
      display: flex;
      align-items: baseline;
      gap: 0.5rem;
      min-width: 0;
      padding: 0.45rem 0.6rem 0;
    }
    .compact-identity .pid { flex: 0 0 auto; }
    .compact-identity .command {
      display: -webkit-box;
      -webkit-box-orient: vertical;
      -webkit-line-clamp: 2;
      line-clamp: 2;
      min-width: 0;
      overflow: hidden;
      white-space: normal;
      overflow-wrap: anywhere;
    }
    .compact-metrics {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: 0.25rem 0.7rem;
      padding: 0 0.6rem 0.45rem;
      color: var(--muted, #8b9bad);
      font: 11px/1.4 var(--mono, monospace);
    }
    .compact-row.tree .compact-identity,
    .compact-row.tree .compact-metrics { padding-left: 0.1rem; }
    .compact-metrics .metric-cpu { color: var(--text, #dbe6f2); }

    .empty {
      margin: 0;
      padding: 1.5rem 0.75rem;
      text-align: center;
      color: var(--muted, #8b9bad);
      border: 1px dashed var(--panel-border, #223041);
      border-radius: 8px;
    }

    @media (max-width: 48rem) {
      .pane.metrics { --metrics-columns: 5.2rem 5.5rem 4.5rem; }
      .pane.identity { --identity-columns: 4rem minmax(8rem, max-content); }
      /* Hidden header and row cells both leave their track, so the remaining
         columns still line up. */
      .col-threads,
      .col-user { display: none; }
      .toolbar .spacer { display: none; }
      .field-label { display: none; }
      .btn-label { display: none; }
      .filters { padding: 0.4rem; }
      button { min-height: 40px; }
      input[type='search'],
      input[type='text'],
      select { min-height: 40px; }
      .view-switch button { min-height: 40px; }
      .table-shell {
        --process-row-height: 30px;
        max-height: 26rem;
      }
      .compact-list { max-height: 26rem; }
      .selection { flex-basis: 100%; }
    }
  `
}

function keyOf(process: ProcessInfo): string {
  return `${process.pid}:${process.start_ticks}`
}

function intervalLabel(ms: number): string {
  return ms >= 1000 ? `${ms / 1000}s` : `${ms}ms`
}

declare global {
  interface HTMLElementTagNameMap {
    'anth-processes': AnthProcesses
  }
}
