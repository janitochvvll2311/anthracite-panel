import { afterEach, describe, expect, it, vi } from 'vitest'
import './anth-process-flat-table'
import type { AnthProcessFlatTable } from './anth-process-flat-table'
import type { ProcessRow } from '../lib/processes/process-client'

function row(overrides: Partial<ProcessRow> = {}): ProcessRow {
  return {
    pid: 42,
    ppid: 1,
    uid: 1000,
    user: 'debian',
    name: 'bash',
    command: '/bin/bash -l',
    state: 'S',
    cpu_ticks: 10,
    rss_kib: 2048,
    threads: 2,
    start_ticks: 99,
    cpu_percent: 12.34,
    ...overrides,
  }
}

async function mount(rows: ProcessRow[]): Promise<AnthProcessFlatTable> {
  const element = document.createElement(
    'anth-process-flat-table',
  ) as AnthProcessFlatTable
  element.rows = rows
  document.body.append(element)
  await element.updateComplete
  return element
}

afterEach(() => {
  document.body.innerHTML = ''
})

describe('AnthProcessFlatTable', () => {
  it('renders every shared column and formats the process metrics', async () => {
    const element = await mount([
      row({
        state: 'R',
        rss_kib: 1536,
        threads: 3,
        cpu_percent: 7.25,
      }),
    ])
    const root = element.shadowRoot

    expect(
      [...(root?.querySelectorAll('thead th') ?? [])].map((header) =>
        header.textContent?.replace(/[↑↓↕]/g, '').trim(),
      ),
    ).toEqual(['PID', 'COMMAND', 'THREADS', 'USER', 'STATE', 'MEM', 'CPU'])

    const cells = [...(root?.querySelectorAll('tbody td') ?? [])].map(
      (cell) => cell.textContent?.trim(),
    )
    expect(cells).toEqual([
      '42',
      '/bin/bash -l',
      '3',
      'debian',
      'running',
      '1.5 MiB',
      '7.3%',
    ])
    expect(root?.querySelector('tbody td.command')?.getAttribute('title')).toBe(
      '/bin/bash -l',
    )
    expect(root?.querySelector('.state')?.getAttribute('data-state')).toBe('R')
  })

  it('shows a dash when CPU has no previous sample and the empty message', async () => {
    const element = await mount([row({ cpu_percent: null })])
    expect(
      element.shadowRoot?.querySelector('tbody td:last-child')?.textContent?.trim(),
    ).toBe('—')

    element.rows = []
    await element.updateComplete
    expect(
      element.shadowRoot?.querySelector('tbody .empty')?.textContent?.trim(),
    ).toBe('No processes.')

    element.emptyMessage = 'Nothing here.'
    await element.updateComplete
    expect(
      element.shadowRoot?.querySelector('tbody .empty')?.textContent?.trim(),
    ).toBe('Nothing here.')
  })

  it('exposes sort state through aria-sort and emits the clicked column', async () => {
    const element = await mount([row()])
    element.sortKey = 'cpu'
    element.sortDirection = 'desc'
    await element.updateComplete

    const headers = [...(element.shadowRoot?.querySelectorAll('thead th') ?? [])]
    expect(headers[6]?.getAttribute('aria-sort')).toBe('descending')
    expect(element.shadowRoot?.querySelectorAll('th[aria-sort="none"]')).toHaveLength(6)

    const listener = vi.fn()
    element.addEventListener('anth-process-sort', listener)
    const memory = headers[5]?.querySelector('button')
    memory?.click()

    expect(listener).toHaveBeenCalledWith(
      expect.objectContaining({ detail: { key: 'memory' } }),
    )
  })

  it('selects rows with a click, Enter or Space and marks the selection', async () => {
    const element = await mount([row()])
    element.selectedPid = 42
    await element.updateComplete

    const selectedRow = element.shadowRoot?.querySelector('tbody tr')
    expect(selectedRow?.classList.contains('selected')).toBe(true)
    expect(selectedRow?.getAttribute('aria-selected')).toBe('true')

    const listener = vi.fn()
    element.addEventListener('anth-process-select', listener)

    selectedRow?.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    expect(listener).toHaveBeenLastCalledWith(
      expect.objectContaining({ detail: { pid: 42 } }),
    )

    selectedRow?.dispatchEvent(
      new KeyboardEvent('keydown', {
        key: 'Enter',
        bubbles: true,
        cancelable: true,
      }),
    )
    expect(listener).toHaveBeenCalledTimes(2)

    selectedRow?.dispatchEvent(
      new KeyboardEvent('keydown', {
        key: ' ',
        bubbles: true,
        cancelable: true,
      }),
    )
    expect(listener).toHaveBeenCalledTimes(3)

    selectedRow?.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'x', bubbles: true }),
    )
    expect(listener).toHaveBeenCalledTimes(3)
  })
})
