import type { ProcessRow } from './process-client'

export type ProcessSortKey =
  | 'pid'
  | 'threads'
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
  { key: 'threads', label: 'THREADS', numeric: true },
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

export { formatKib } from '../format'

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
    case 'threads':
      return process.threads
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

/**
 * Splits a typed command line into the program and its arguments.
 *
 * The form takes one line because that is what an operator types; the daemon
 * takes an argument vector because a shell would re-split it. Splitting here
 * keeps the two halves honest without ever handing a string to a shell.
 */
export function parseCommandLine(input: string): {
  program: string
  args: string[]
} {
  const parts = splitCommandLine(input)
  const [program = '', ...args] = parts
  return { program, args }
}

/**
 * Quotes-aware splitter: single and double quotes group words, a backslash
 * escapes the next character outside single quotes.
 *
 * A dangling quote is not an error to the operator mid-typing, so it simply
 * keeps the rest of the line as one word.
 */
export function splitCommandLine(input: string): string[] {
  const words: string[] = []
  let current = ''
  let started = false
  let quote: '"' | "'" | null = null

  for (let index = 0; index < input.length; index += 1) {
    const character = input[index]

    if (quote !== null) {
      if (character === quote) {
        quote = null
        continue
      }
      current += character
      continue
    }

    if (character === '"' || character === "'") {
      quote = character
      started = true
      continue
    }

    if (character === '\\' && index + 1 < input.length) {
      index += 1
      current += input[index]
      started = true
      continue
    }

    if (character === ' ' || character === '\t') {
      if (started) {
        words.push(current)
        current = ''
        started = false
      }
      continue
    }

    current += character
    started = true
  }

  if (started) words.push(current)
  return words
}

/**
 * The reason a spawn cannot be offered, or `undefined` when it can.
 *
 * The daemon is still the authority; this only stops the UI presenting a form
 * whose submission is certain to be refused.
 */
export function spawnBlockReason(snapshot: {
  spawn_enabled: boolean
  spawn_allow: string[]
}): string | undefined {
  if (!snapshot.spawn_enabled) return 'Process spawning is disabled by the daemon.'
  if (snapshot.spawn_allow.length === 0) {
    return 'The daemon has no programs in `spawn_allow`, so every spawn is denied.'
  }
  return undefined
}

/** Validates the spawn form and returns the normalised request fields. */
export function parseSpawnForm(form: {
  command: string
  cwd: string
}): { program: string; args: string[]; cwd?: string } | { error: string } {
  const { program, args } = parseCommandLine(form.command.trim())

  if (program === '') return { error: 'Enter a program to start.' }
  if (!program.startsWith('/')) {
    return { error: 'The program must be an absolute path, for example /bin/ls.' }
  }

  const cwd = form.cwd.trim()
  if (cwd !== '' && !cwd.startsWith('/')) {
    return { error: 'The working directory must be an absolute path.' }
  }

  return { program, args, ...(cwd === '' ? {} : { cwd }) }
}
