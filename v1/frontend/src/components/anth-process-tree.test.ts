import { afterEach, describe, expect, it, vi } from 'vitest'
import './anth-process-tree'
import type { AnthProcessTree } from './anth-process-tree'
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
    cpu_percent: 0,
    ...overrides,
  }
}

function nodes(): ProcessTreeNode[] {
  return [
    {
      process: row({ pid: 1, command: '/sbin/init', cpu_percent: 1 }),
      children: [
        {
          process: row({
            pid: 10,
            ppid: 1,
            command: '/bin/bash -l',
            cpu_percent: 15,
          }),
          children: [
            {
              process: row({
                pid: 100,
                ppid: 10,
                command: 'vim README.md',
                cpu_percent: 5,
              }),
              children: [],
            },
          ],
        },
        {
          process: row({
            pid: 11,
            ppid: 1,
            command: 'sleep 30',
            cpu_percent: 2,
          }),
          children: [],
        },
      ],
    },
  ]
}

async function mount(tree: ProcessTreeNode[]): Promise<AnthProcessTree> {
  const element = document.createElement(
    'anth-process-tree',
  ) as AnthProcessTree
  element.nodes = tree
  document.body.append(element)
  await element.updateComplete
  return element
}

function renderedRows(element: AnthProcessTree): HTMLElement[] {
  return [
    ...(element.shadowRoot?.querySelectorAll<HTMLElement>('.tree .row') ?? []),
  ]
}

afterEach(() => {
  document.body.innerHTML = ''
})

describe('AnthProcessTree', () => {
  it('renders nested rows with guides, levels and a root spacer', async () => {
    const element = await mount(nodes())
    const rows = renderedRows(element)

    expect(rows).toHaveLength(4)
    expect(rows.map((item) => item.getAttribute('aria-level'))).toEqual([
      '1',
      '2',
      '3',
      '2',
    ])

    expect(element.shadowRoot?.querySelectorAll('.tree-vline')).toHaveLength(4)
    expect(element.shadowRoot?.querySelectorAll('.tree-elbow')).toHaveLength(3)
    expect(element.shadowRoot?.querySelectorAll('.tree-root-spacer')).toHaveLength(
      1,
    )
    expect(element.shadowRoot?.querySelectorAll('.toggle')).toHaveLength(2)
    expect(
      element.shadowRoot?.querySelectorAll('.toggle-spacer'),
    ).toHaveLength(2)

    expect(rows[0]?.querySelector('.tree-elbow')).toBeNull()
    expect(rows[1]?.querySelector('.tree-vline.last')).not.toBeNull()
    expect(rows[2]?.querySelectorAll('.tree-vline')).toHaveLength(2)
  })

  it('shows the process metrics and exposes the full command through the title', async () => {
    const element = await mount(nodes())
    const child = renderedRows(element)[1]

    expect(child?.querySelector('.command')?.textContent?.trim()).toBe(
      '/bin/bash -l',
    )
    expect(child?.querySelector('.command')?.getAttribute('title')).toBe(
      '/bin/bash -l',
    )
    expect(child?.querySelectorAll('.cell')).toHaveLength(8)
  })

  it('collapses and expands a branch without selecting the row', async () => {
    const element = await mount(nodes())
    const selected = vi.fn()
    element.addEventListener('anth-process-select', selected)

    const rootToggle = renderedRows(element)[0]?.querySelector<HTMLButtonElement>(
      '.toggle',
    )
    expect(rootToggle?.getAttribute('aria-expanded')).toBe('true')
    rootToggle?.click()
    await element.updateComplete

    expect(renderedRows(element)).toHaveLength(1)
    expect(selected).not.toHaveBeenCalled()
    expect(rootToggle?.getAttribute('aria-expanded')).toBe('false')

    rootToggle?.click()
    await element.updateComplete
    expect(renderedRows(element)).toHaveLength(4)

    const childToggle = renderedRows(element)[1]?.querySelector<HTMLButtonElement>(
      '.toggle',
    )
    childToggle?.click()
    await element.updateComplete
    expect(renderedRows(element)).toHaveLength(3)
    expect(
      renderedRows(element)[1]?.getAttribute('aria-expanded'),
    ).toBe('false')
  })

  it('only exposes the sortable headers allowed in tree mode', async () => {
    const element = await mount(nodes())
    const headers = [
      ...(element.shadowRoot?.querySelectorAll('.head > [role="columnheader"]') ?? []),
    ]

    expect(headers).toHaveLength(8)
    expect(headers[0]?.textContent?.trim()).toBe('TREE')
    expect(headers[1]?.textContent?.trim()).toBe('')
    expect(headers[2]?.textContent?.trim()).toBe('')
    expect(
      headers.slice(3).map((header) =>
        header.textContent?.replace(/[↑↓↕]/g, '').trim(),
      ),
    ).toEqual(['THREADS', 'USER', 'STATE', 'MEM', 'CPU'])

    const listener = vi.fn()
    element.addEventListener('anth-process-sort', listener)
    headers[3]?.querySelector('button')?.click()
    expect(listener).toHaveBeenCalledWith(
      expect.objectContaining({ detail: { key: 'threads' } }),
    )
  })

  it('reports the active sort direction in the header', async () => {
    const element = await mount(nodes())
    element.sortKey = 'memory'
    element.sortDirection = 'asc'
    await element.updateComplete

    const memory = [
      ...(element.shadowRoot?.querySelectorAll('.head .head-cell') ?? []),
    ].find((header) => header.textContent?.includes('MEM'))
    expect(memory?.getAttribute('aria-sort')).toBe('ascending')
  })

  it('selects a row by click or keyboard and shows the empty message', async () => {
    const element = await mount(nodes())
    element.selectedPid = 10
    await element.updateComplete

    const child = renderedRows(element)[1]
    expect(child?.classList.contains('selected')).toBe(true)
    expect(child?.getAttribute('aria-selected')).toBe('true')

    const listener = vi.fn()
    element.addEventListener('anth-process-select', listener)
    child?.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    expect(listener).toHaveBeenLastCalledWith(
      expect.objectContaining({ detail: { pid: 10 } }),
    )

    child?.dispatchEvent(
      new KeyboardEvent('keydown', {
        key: ' ',
        bubbles: true,
        cancelable: true,
      }),
    )
    expect(listener).toHaveBeenCalledTimes(2)

    element.nodes = []
    await element.updateComplete
    expect(
      element.shadowRoot?.querySelector('.empty')?.textContent?.trim(),
    ).toBe('No processes.')
  })
})
