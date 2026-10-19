import type { ProcessRow } from './process-client'

export type ProcessSortKey =
  | 'pid'
  | 'user'
  | 'cpu'
  | 'memory'
  | 'state'
  | 'command'

export type ProcessSortDirection = 'asc' | 'desc'
export type ProcessView = 'flat' | 'tree'

export interface ProcessColumn {
  key: ProcessSortKey
  label: string
  numeric: boolean
}

/** Data columns shared by the flat table and the tree header. */
export const PROCESS_COLUMNS: ProcessColumn[] = [
  { key: 'pid', label: 'PID', numeric: true },
  { key: 'command', label: 'COMMAND', numeric: false },
  { key: 'user', label: 'USER', numeric: false },
  { key: 'state', label: 'STATE', numeric: false },
  { key: 'memory', label: 'MEM', numeric: true },
  { key: 'cpu', label: 'CPU', numeric: true },
]

export const ANTH_PROCESS_SELECT_EVENT = 'anth-process-select'
export const ANTH_PROCESS_SORT_EVENT = 'anth-process-sort'

export interface AnthProcessSelectDetail {
  pid: number
}

export interface AnthProcessSortDetail {
  key: ProcessSortKey
}

/** One rendered tree row, already ordered and with its connector prefix. */
export interface ProcessTreeRow {
  process: ProcessRow
  depth: number
  prefix: string
}

/** Nested tree node consumed by `anth-process-tree`. */
export interface ProcessTreeNode {
  process: ProcessRow
  children: ProcessTreeNode[]
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

export function stateLabel(state: string): string {
  return STATE_LABELS[state] ?? state
}

export function formatKib(kib: number): string {
  const mib = kib / 1024
  if (mib >= 1024) return `${(mib / 1024).toFixed(1)} GiB`
  if (mib >= 1) return `${mib.toFixed(1)} MiB`
  return `${Math.round(kib)} KiB`
}

/** Numeric metrics default to descending; identifiers and names to ascending. */
export function defaultDirection(key: ProcessSortKey): ProcessSortDirection {
  return key === 'cpu' || key === 'memory' ? 'desc' : 'asc'
}

export function sortValue(
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
    case 'user':
      return process.user
    case 'state':
      return process.state
    case 'command':
      return process.command === '' ? process.name : process.command
  }
}

export function compareProcesses(
  left: ProcessRow,
  right: ProcessRow,
  key: ProcessSortKey,
  direction: ProcessSortDirection,
): number {
  const a = sortValue(left, key)
  const b = sortValue(right, key)
  const order = direction === 'asc' ? 1 : -1

  if (a === null && b === null) return left.pid - right.pid
  if (a === null) return 1
  if (b === null) return -1
  if (typeof a === 'string' && typeof b === 'string') {
    return a.localeCompare(b) * order
  }
  return (Number(a) - Number(b)) * order
}
