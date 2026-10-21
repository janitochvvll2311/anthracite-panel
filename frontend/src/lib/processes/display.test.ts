import { describe, expect, it } from 'vitest'
import type { ProcessRow } from './process-client'
import {
  PROCESS_COLUMNS,
  compareProcesses,
  defaultDirection,
  formatKib,
  parseCommandLine,
  parseSpawnForm,
  sortValue,
  spawnBlockReason,
  splitCommandLine,
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

  it('splits a command line without ever invoking a shell', () => {
    expect(splitCommandLine('/bin/ls -la /tmp')).toEqual([
      '/bin/ls',
      '-la',
      '/tmp',
    ])
    // Quotes group, and the quotes themselves do not survive into the argv the
    // daemon receives.
    expect(splitCommandLine('/bin/echo "hello there"')).toEqual([
      '/bin/echo',
      'hello there',
    ])
    expect(splitCommandLine("/bin/echo 'a b' c")).toEqual([
      '/bin/echo',
      'a b',
      'c',
    ])
    // A backslash escapes the next character outside single quotes.
    expect(splitCommandLine('/bin/echo a\\ b')).toEqual(['/bin/echo', 'a b'])
    // An empty argument is still an argument, which a naive filter would drop.
    expect(splitCommandLine('/bin/echo ""')).toEqual(['/bin/echo', ''])
  })

  it('drops no argument when splitting a typed command', () => {
    expect(parseCommandLine('/usr/bin/uptime --pretty')).toEqual({
      program: '/usr/bin/uptime',
      args: ['--pretty'],
    })
    expect(parseCommandLine('/usr/bin/uptime')).toEqual({
      program: '/usr/bin/uptime',
      args: [],
    })
  })

  it('refuses a spawn form that the daemon would refuse', () => {
    expect(parseSpawnForm({ command: '', cwd: '' })).toEqual({
      error: 'Enter a program to start.',
    })
    expect(parseSpawnForm({ command: 'echo hi', cwd: '' })).toEqual({
      error: 'The program must be an absolute path, for example /bin/ls.',
    })
    expect(parseSpawnForm({ command: '/bin/ls', cwd: 'tmp' })).toEqual({
      error: 'The working directory must be an absolute path.',
    })
  })

  it('omits an empty working directory instead of sending an empty string', () => {
    expect(parseSpawnForm({ command: '/bin/ls -la', cwd: '  ' })).toEqual({
      program: '/bin/ls',
      args: ['-la'],
    })
    expect(parseSpawnForm({ command: '/bin/ls', cwd: '/tmp' })).toEqual({
      program: '/bin/ls',
      args: [],
      cwd: '/tmp',
    })
  })

  it('blocks spawning when the daemon switch is off or the allowlist is empty', () => {
    expect(
      spawnBlockReason({ spawn_enabled: false, spawn_allow: ['/bin/ls'] }),
    ).toContain('disabled')
    // The empty allowlist is its own message: an operator who enabled the
    // switch needs to know why nothing is allowed.
    expect(
      spawnBlockReason({ spawn_enabled: true, spawn_allow: [] }),
    ).toContain('spawn_allow')
    expect(
      spawnBlockReason({ spawn_enabled: true, spawn_allow: ['/bin/ls'] }),
    ).toBeUndefined()
  })
})
