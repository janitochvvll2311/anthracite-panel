/** Shared display logic for the flat process table and the process tree. */

import { msg } from '@lit/localize'

import type { ProcessInfo } from 'anth-wire'

/** A row with its CPU percentage already computed from two snapshots. */
export interface ProcessRow extends ProcessInfo {
  cpu_percent: number | null
}

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
export function processColumns(): ProcessColumn[] {
  return [
    { key: 'pid', label: msg('PID'), numeric: true },
    { key: 'command', label: msg('Command'), numeric: false },
    { key: 'threads', label: msg('Threads'), numeric: true },
    { key: 'user', label: msg('User'), numeric: false },
    { key: 'state', label: msg('State'), numeric: false },
    { key: 'memory', label: msg('Memory'), numeric: true },
    { key: 'cpu', label: msg('CPU'), numeric: true },
  ]
}

/** Human name for a one-letter `/proc` state. */
export function stateLabel(state: string): string {
  switch (state) {
    case 'R':
      return msg('Running')
    case 'S':
      return msg('Sleeping')
    case 'D':
      return msg('Disk')
    case 'Z':
      return msg('Zombie')
    case 'T':
      return msg('Stopped')
    case 't':
      return msg('Tracing')
    case 'I':
      return msg('Idle')
    case 'X':
      return msg('Dead')
    default:
      return state
  }
}

/** Numeric metrics default to descending; identifiers and names to ascending. */
export function defaultDirection(key: ProcessSortKey): ProcessSortDirection {
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

/** Stable ordering behind both views: the selected key, then pid. */
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

/** One nested node consumed by the tree renderer. */
export interface ProcessTreeNode {
  process: ProcessRow
  children: ProcessTreeNode[]
}

/**
 * Nests rows by `ppid`, sorting siblings with the same comparator as the flat
 * table. Roots are processes whose parent is not in the visible set (the
 * filter may hide it), plus pid 0/1 boundaries; cycles are shown at depth zero
 * instead of disappearing.
 */
export function buildTree(
  rows: ProcessRow[],
  key: ProcessSortKey,
  direction: ProcessSortDirection,
): ProcessTreeNode[] {
  const byPid = new Map(rows.map((process) => [process.pid, process]))
  const children = new Map<number, ProcessRow[]>()
  const roots: ProcessRow[] = []

  for (const process of rows) {
    if (process.ppid === process.pid || !byPid.has(process.ppid)) {
      roots.push(process)
      continue
    }
    const siblings = children.get(process.ppid) ?? []
    siblings.push(process)
    children.set(process.ppid, siblings)
  }

  const visited = new Set<number>()

  const sortRows = (list: ProcessRow[]) =>
    [...list].sort((left, right) =>
      compareProcesses(left, right, key, direction),
    )

  const build = (process: ProcessRow, path: Set<number>): ProcessTreeNode => {
    visited.add(process.pid)
    path.add(process.pid)
    const descendants = sortRows(children.get(process.pid) ?? []).filter(
      (child) => !path.has(child.pid),
    )
    return {
      process,
      children: descendants.map((child) => build(child, new Set(path))),
    }
  }

  const nodes = sortRows(roots).map((root) => build(root, new Set()))

  // A malformed parent cycle has no natural root; show it at depth zero.
  for (const process of rows) {
    if (!visited.has(process.pid)) {
      nodes.push(build(process, new Set()))
    }
  }
  return nodes
}

/** A flat, already ordered list of tree rows with their connector state. */
export interface TreeDisplayRow {
  process: ProcessRow
  depth: number
  ancestorsLast: boolean[]
  isLast: boolean
  hasChildren: boolean
  expanded: boolean
}

/** Flattens the tree, dropping the children of collapsed nodes. */
export function visibleTreeRows(
  nodes: ProcessTreeNode[],
  collapsed: ReadonlySet<number>,
): TreeDisplayRow[] {
  const rows: TreeDisplayRow[] = []
  const walk = (
    node: ProcessTreeNode,
    depth: number,
    ancestorsLast: boolean[],
    isLast: boolean,
  ) => {
    const expanded = !collapsed.has(node.process.pid)
    rows.push({
      process: node.process,
      depth,
      ancestorsLast,
      isLast,
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
  nodes.forEach((node, index) => {
    walk(node, 0, [], index === nodes.length - 1)
  })
  return rows
}

/**
 * Splits a command line into a program and its arguments.
 *
 * Whitespace separates them, quotes group them and a backslash escapes outside
 * single quotes. There is no shell: nothing is expanded or executed.
 */
export function parseCommandLine(input: string): {
  program: string
  args: string[]
} {
  const [program = '', ...args] = splitCommandLine(input)
  return { program, args }
}

/** Quotes-aware splitter used by the spawn form. */
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
      if (quote === '"' && character === '\\' && index + 1 < input.length) {
        index += 1
        current += input[index]
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
    if (character === ' ' || character === '\t' || character === '\n') {
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
