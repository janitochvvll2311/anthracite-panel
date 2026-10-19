import { describe, expect, it } from 'vitest'
import type { ProcessRow } from './process-client'
import {
  PROCESS_COLUMNS,
  compareProcesses,
  defaultDirection,
  formatKib,
  sortValue,
  stateLabel,
} from './display'

function row(overrides: Partial<ProcessRow> = {}): ProcessRow {
  return {
    pid: 1,
    ppid: 0,
    uid: 1000,
    user: 'debian',
    name: 'init',
    command: '/sbin/init',
    state: 'S',
    cpu_ticks: 0,
    rss_kib: 1024,
    threads: 1,
    start_ticks: 10,
    cpu_percent: 0,
    ...overrides,
  }
}

describe('process display helpers', () => {
  it('exposes the shared data columns in display order', () => {
    expect(PROCESS_COLUMNS.map((column) => column.label)).toEqual([
      'PID',
      'COMMAND',
      'THREADS',
      'USER',
      'STATE',
      'MEM',
      'CPU',
    ])
  })

  it('translates known process states and preserves unknown ones', () => {
    expect(stateLabel('R')).toBe('running')
    expect(stateLabel('S')).toBe('sleeping')
    expect(stateLabel('Z')).toBe('zombie')
    expect(stateLabel('?')).toBe('?')
  })

  it('formats KiB, MiB and GiB values', () => {
    expect(formatKib(512)).toBe('512 KiB')
    expect(formatKib(2048)).toBe('2.0 MiB')
    expect(formatKib(3 * 1024 * 1024)).toBe('3.0 GiB')
  })

  it('defaults metrics to descending and identifiers to ascending', () => {
    expect(defaultDirection('cpu')).toBe('desc')
    expect(defaultDirection('memory')).toBe('desc')
    expect(defaultDirection('pid')).toBe('asc')
    expect(defaultDirection('threads')).toBe('asc')
    expect(defaultDirection('command')).toBe('asc')
  })

  it('falls back to the process name when the command is empty', () => {
    expect(sortValue(row({ command: '' }), 'command')).toBe('init')
    expect(sortValue(row({ command: '/bin/bash' }), 'command')).toBe(
      '/bin/bash',
    )
    expect(sortValue(row({ cpu_percent: null }), 'cpu')).toBeNull()
  })

  it('compares numbers and strings in both directions', () => {
    const low = row({ pid: 10, user: 'alice', rss_kib: 10 })
    const high = row({ pid: 20, user: 'bob', rss_kib: 20 })

    expect(compareProcesses(low, high, 'pid', 'asc')).toBeLessThan(0)
    expect(compareProcesses(low, high, 'pid', 'desc')).toBeGreaterThan(0)
    expect(compareProcesses(low, high, 'user', 'asc')).toBeLessThan(0)
    expect(compareProcesses(low, high, 'user', 'desc')).toBeGreaterThan(0)
  })

  it('sorts unknown CPU values last and breaks null ties by pid', () => {
    const unknown = row({ pid: 3, cpu_percent: null })
    const known = row({ pid: 2, cpu_percent: 1 })

    expect(compareProcesses(known, unknown, 'cpu', 'desc')).toBeLessThan(0)
    expect(compareProcesses(unknown, known, 'cpu', 'asc')).toBeGreaterThan(0)
    expect(
      compareProcesses(
        row({ pid: 5, cpu_percent: null }),
        row({ pid: 4, cpu_percent: null }),
        'cpu',
        'desc',
      ),
    ).toBe(1)
    expect(
      compareProcesses(
        row({ pid: 5, threads: 2 }),
        row({ pid: 4, threads: 2 }),
        'threads',
        'asc',
      ),
    ).toBe(0)
  })
})
