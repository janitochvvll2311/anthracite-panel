import { expect, test } from 'vitest'

import {
  buildTree,
  compareProcesses,
  defaultDirection,
  parseCommandLine,
  splitCommandLine,
  stateLabel,
  visibleTreeRows,
} from './lib/processes/display'
import type { ProcessRow } from './lib/processes/display'

function row(
  pid: number,
  ppid: number,
  overrides: Partial<ProcessRow> = {},
): ProcessRow {
  return {
    pid,
    ppid,
    uid: 0,
    user: 'root',
    name: `p${pid}`,
    command: `p${pid}`,
    state: 'S',
    cpu_ticks: 0,
    rss_kib: 0,
    threads: 1,
    start_ticks: pid * 10,
    cpu_percent: null,
    ...overrides,
  }
}

test('splits a command line without a shell', () => {
  expect(splitCommandLine('alpha beta\tgamma')).toEqual(['alpha', 'beta', 'gamma'])
  expect(splitCommandLine('$HOME && echo hi')).toEqual(['$HOME', '&&', 'echo', 'hi'])
  expect(splitCommandLine('--message "hello world"')).toEqual([
    '--message',
    'hello world',
  ])
  expect(splitCommandLine('one "" two')).toEqual(['one', '', 'two'])
  expect(splitCommandLine('a\\ b c')).toEqual(['a b', 'c'])
  expect(parseCommandLine('/bin/sleep 30')).toEqual({
    program: '/bin/sleep',
    args: ['30'],
  })
  expect(parseCommandLine('')).toEqual({ program: '', args: [] })
})

test('sorts null percentages last and strings alphabetically', () => {
  const rows = [
    row(2, 1, { cpu_percent: 5, user: 'zed' }),
    row(1, 0, { cpu_percent: null, user: 'alice' }),
    row(3, 1, { cpu_percent: 15, user: 'bob' }),
  ]
  const byCpu = [...rows].sort((a, b) => compareProcesses(a, b, 'cpu', 'desc'))
  expect(byCpu.map((process) => process.pid)).toEqual([3, 2, 1])
  const byUser = [...rows].sort((a, b) => compareProcesses(a, b, 'user', 'asc'))
  expect(byUser.map((process) => process.user)).toEqual(['alice', 'bob', 'zed'])
  expect(defaultDirection('cpu')).toBe('desc')
  expect(defaultDirection('pid')).toBe('asc')
})

test('builds a tree by ppid and sorts siblings', () => {
  const rows = [
    row(1, 0),
    row(10, 1, { cpu_percent: 1 }),
    row(11, 1, { cpu_percent: 9 }),
    row(12, 11, { cpu_percent: 3 }),
    row(20, 999),
  ]
  const tree = buildTree(rows, 'cpu', 'desc')
  expect(tree.map((node) => node.process.pid)).toEqual([1, 20])
  const root = tree[0]
  expect(root.children.map((node) => node.process.pid)).toEqual([11, 10])
  expect(root.children[0].children.map((node) => node.process.pid)).toEqual([12])
})

test('flattening the tree honours the collapsed set', () => {
  const rows = [row(1, 0), row(2, 1), row(3, 2), row(4, 1)]
  const nodes = buildTree(rows, 'pid', 'asc')

  const open = visibleTreeRows(nodes, new Set())
  expect(open.map((entry) => [entry.process.pid, entry.depth])).toEqual([
    [1, 0],
    [2, 1],
    [3, 2],
    [4, 1],
  ])
  expect(open[1].hasChildren).toBe(true)
  expect(open[2].ancestorsLast).toEqual([true, false])

  const collapsed = visibleTreeRows(nodes, new Set([2]))
  expect(collapsed.map((entry) => entry.process.pid)).toEqual([1, 2, 4])
  expect(collapsed[1].expanded).toBe(false)
})

test('a parent cycle is shown at depth zero instead of disappearing', () => {
  const nodes = buildTree([row(5, 6), row(6, 5)], 'pid', 'asc')
  const rows = visibleTreeRows(nodes, new Set())
  expect(rows.map((entry) => entry.process.pid).sort()).toEqual([5, 6])
})

test('states have readable labels', () => {
  expect(stateLabel('R')).toBe('Running')
  expect(stateLabel('Z')).toBe('Zombie')
  expect(stateLabel('?')).toBe('?')
})
