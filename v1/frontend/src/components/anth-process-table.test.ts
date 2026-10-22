import { afterEach, describe, expect, it } from 'vitest'
import './anth-process-table'
import type { AnthProcessTable } from './anth-process-table'
import type { ProcessRow } from '../lib/processes/process-client'
import type { ProcessTreeNode } from '../lib/processes/display'

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
    cpu_percent: 10,
    ...overrides,
  }
}

async function mount(processes: ProcessRow[]): Promise<AnthProcessTable> {
  const element = document.createElement(
    'anth-process-table',
  ) as AnthProcessTable
  element.processes = processes
  document.body.append(element)
  await element.updateComplete
  return element
}

async function settle(element: AnthProcessTable): Promise<void> {
  await element.updateComplete
  const flat = element.shadowRoot?.querySelector('anth-process-flat-table')
  const tree = element.shadowRoot?.querySelector('anth-process-tree')
  await flat?.updateComplete
  await tree?.updateComplete
  await element.updateComplete
}

function sample(): ProcessRow[] {
  return [
    row({ pid: 1, ppid: 0, command: '/sbin/init', cpu_percent: 10 }),
    row({ pid: 2, ppid: 1, command: '/bin/bash', cpu_percent: 30 }),
    row({ pid: 3, ppid: 0, command: 'sleep 60', cpu_percent: 20 }),
  ]
}

afterEach(() => {
  document.body.innerHTML = ''
})

describe('AnthProcessTable', () => {
  it('sorts the flat view by CPU descending by default', async () => {
    const element = await mount(sample())
    const flat = element.shadowRoot?.querySelector(
      'anth-process-flat-table',
    ) as (HTMLElement & { rows: ProcessRow[] }) | null

    expect(flat?.rows.map((process) => process.pid)).toEqual([2, 3, 1])
    expect(
      flat?.shadowRoot?.querySelectorAll('tbody tr')[0]?.textContent,
    ).toContain('/bin/bash')
  })

  it('toggles a header sort and uses the default direction for a new key', async () => {
    const element = await mount(sample())
    const flat = element.shadowRoot?.querySelector(
      'anth-process-flat-table',
    ) as (HTMLElement & { rows: ProcessRow[] }) | null

    const pidHeader = flat?.shadowRoot?.querySelector<HTMLButtonElement>(
      'thead th:first-child button',
    )
    pidHeader?.click()
    await settle(element)
    expect(flat?.rows.map((process) => process.pid)).toEqual([1, 2, 3])

    pidHeader?.click()
    await settle(element)
    expect(flat?.rows.map((process) => process.pid)).toEqual([3, 2, 1])

    const threadsHeader = flat?.shadowRoot?.querySelector<HTMLButtonElement>(
      'thead th:nth-child(3) button',
    )
    threadsHeader?.click()
    await settle(element)
    expect(
      flat?.shadowRoot
        ?.querySelector('thead th:nth-child(3)')
        ?.getAttribute('aria-sort'),
    ).toBe('ascending')
  })

  it('renders the tree and resets PID/COMMAND sorting when switching views', async () => {
    const element = await mount(sample())
    const flat = element.shadowRoot?.querySelector(
      'anth-process-flat-table',
    ) as (HTMLElement & { rows: ProcessRow[] }) | null

    flat?.shadowRoot
      ?.querySelector<HTMLButtonElement>('thead th:first-child button')
      ?.click()
    await settle(element)
    expect(flat?.rows.map((process) => process.pid)).toEqual([1, 2, 3])

    element.view = 'tree'
    await settle(element)

    const tree = element.shadowRoot?.querySelector(
      'anth-process-tree',
    ) as (HTMLElement & { nodes: ProcessTreeNode[]; sortKey: string }) | null
    expect(tree?.sortKey).toBe('cpu')
    expect(tree?.nodes.map((node) => node.process.pid)).toEqual([3, 1])
    expect(tree?.nodes[1]?.children.map((node) => node.process.pid)).toEqual([
      2,
    ])
    expect(flat?.isConnected).toBe(false)
  })

  it('keeps PID/COMMAND sorting in the flat view', async () => {
    const element = await mount(sample())
    element.view = 'flat'
    await settle(element)
    const flat = element.shadowRoot?.querySelector(
      'anth-process-flat-table',
    ) as (HTMLElement & { rows: ProcessRow[] }) | null

    flat?.shadowRoot
      ?.querySelector<HTMLButtonElement>('thead th:nth-child(2) button')
      ?.click()
    await settle(element)
    expect(flat?.rows.map((process) => process.command)).toEqual([
      '/bin/bash',
      '/sbin/init',
      'sleep 60',
    ])
  })

  it('renders the warning and empty message', async () => {
    const element = await mount(sample())
    element.warning = 'showing 3 of 500 processes'
    await element.updateComplete
    expect(
      element.shadowRoot?.querySelector('.warning')?.textContent?.trim(),
    ).toBe('showing 3 of 500 processes')

    element.processes = []
    element.emptyMessage = 'No clients.'
    await settle(element)
    const flat = element.shadowRoot?.querySelector('anth-process-flat-table')
    expect(flat?.shadowRoot?.querySelector('.empty')?.textContent?.trim()).toBe(
      'No clients.',
    )
  })

  it('tracks selection and forwards it from either renderer', async () => {
    const element = await mount(sample())
    const flat = element.shadowRoot?.querySelector(
      'anth-process-flat-table',
    ) as (HTMLElement & { rows: ProcessRow[] }) | null

    const second = flat?.shadowRoot?.querySelectorAll<HTMLElement>('tbody tr')[1]
    second?.click()
    await settle(element)
    expect(element.selectedPid).toBe(3)
    expect(
      element.shadowRoot
        ?.querySelector('anth-process-flat-table')
        ?.shadowRoot?.querySelector('tbody tr.selected')
        ?.textContent,
    ).toContain('sleep 60')
  })

  it('shows self-parented, orphaned and cyclic processes at the root', async () => {
    const element = await mount([
      row({ pid: 4, ppid: 4, command: 'self' }),
      row({ pid: 5, ppid: 999, command: 'orphan' }),
      row({ pid: 6, ppid: 7, command: 'cycle-a' }),
      row({ pid: 7, ppid: 6, command: 'cycle-b' }),
    ])
    element.view = 'tree'
    await settle(element)

    const tree = element.shadowRoot?.querySelector(
      'anth-process-tree',
    ) as (HTMLElement & { nodes: ProcessTreeNode[] }) | null
    expect(tree?.nodes.map((node) => node.process.pid).sort()).toEqual([
      4, 5, 6,
    ])
    expect(tree?.nodes[2]?.children.map((node) => node.process.pid)).toEqual([
      7,
    ])
  })
})
