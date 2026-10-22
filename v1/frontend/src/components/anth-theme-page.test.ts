import { afterEach, describe, expect, it, vi } from 'vitest'

import './anth-theme-page'
import type { AnthThemePage } from './anth-theme-page'
import { ANTH_PANEL_SELECT_EVENT } from './anth-theme-page'
import type { AnthTabPanel } from './anth-tab-panel'
import type { PanelDefinition } from '../app/panels'
import { THEMES, panelsIn } from '../app/panels'

function stubPanel(panel: PanelDefinition): HTMLElement {
  const element = document.createElement('div')
  element.dataset.panel = panel.id
  element.textContent = panel.id
  return element
}

async function settle(page: AnthThemePage): Promise<AnthTabPanel> {
  for (let i = 0; i < 5; i++) {
    await Promise.resolve()
    await new Promise((resolve) => setTimeout(resolve, 0))
    await page.updateComplete
    const tabs = page.shadowRoot?.querySelector<AnthTabPanel>(
      'anth-tab-panel',
    )
    if (tabs) {
      await tabs.updateComplete
      return tabs
    }
  }
  throw new Error('the tab panel was not built')
}

function tabButton(
  tabs: AnthTabPanel,
  id: string,
): HTMLButtonElement {
  const button = tabs.shadowRoot?.querySelector<HTMLButtonElement>(
    `[role="tab"][data-tab="${id}"]`,
  )
  if (!button) throw new Error(`no tab button for ${id}`)
  return button
}

async function mount(
  active = 'host-info',
): Promise<{
  page: AnthThemePage
  tabs: AnthTabPanel
  createPanel: ReturnType<typeof vi.fn>
}> {
  const page = document.createElement('anth-theme-page') as AnthThemePage
  const createPanel = vi.fn((panel: PanelDefinition) => stubPanel(panel))
  page.theme = THEMES.find((theme) => theme.id === 'system')
  page.tabs = panelsIn('system').map((panel) => ({
    id: panel.id,
    label: panel.title(),
    create: () => createPanel(panel),
  }))
  page.active = active
  document.body.append(page)
  await page.updateComplete
  const tabs = await settle(page)
  return { page, tabs, createPanel }
}

afterEach(() => {
  document.body.innerHTML = ''
  vi.restoreAllMocks()
})

describe('AnthThemePage', () => {
  it('renders the theme heading and one tab per panel', async () => {
    const { page, tabs } = await mount()
    const buttons = [...(tabs.shadowRoot?.querySelectorAll('[role="tab"]') ?? [])]

    expect(page.shadowRoot?.querySelector('.title')?.textContent).toBe('System')
    expect(buttons.map((button) => button.textContent?.trim())).toEqual([
      'Host info',
      'Processes',
    ])
  })

  it('mounts the active tab and parks the others', async () => {
    const { tabs } = await mount('host-info')

    expect(tabs.tab('host-info')?.isConnected).toBe(true)
    expect(tabs.tab('processes')?.isConnected).toBe(false)
  })

  it('reports a tab selection and swaps the mounted panel', async () => {
    const { page, tabs } = await mount('host-info')
    const selected: string[] = []
    page.addEventListener(ANTH_PANEL_SELECT_EVENT, (event) => {
      selected.push((event as CustomEvent<{ panel: string }>).detail.panel)
    })

    tabButton(tabs, 'processes').click()
    await settle(page)

    expect(selected).toEqual(['processes'])
    expect(tabs.tab('processes')?.isConnected).toBe(true)
    expect(tabs.tab('host-info')?.isConnected).toBe(false)
  })

  it('follows an external active change', async () => {
    const { page, tabs } = await mount('host-info')

    page.active = 'processes'
    await page.updateComplete
    await tabs.updateComplete

    expect(tabs.active).toBe('processes')
    expect(tabs.tab('processes')?.isConnected).toBe(true)
    expect(tabs.tab('host-info')?.isConnected).toBe(false)
  })

  it('builds each panel element once across tab changes', async () => {
    const { page, tabs, createPanel } = await mount('host-info')

    tabButton(tabs, 'processes').click()
    await settle(page)
    tabButton(tabs, 'host-info').click()
    await settle(page)

    expect(createPanel).toHaveBeenCalledTimes(2)
    expect(
      createPanel.mock.calls.map(
        ([panel]) => (panel as PanelDefinition).id,
      ),
    ).toEqual(['host-info', 'processes'])
  })

  it('falls back to the first panel when the active id is unknown', async () => {
    const { tabs } = await mount('does-not-exist')

    expect(tabs.active).toBe('host-info')
    expect(tabs.tab('host-info')?.isConnected).toBe(true)
  })

  it('renders an empty theme without tabs', async () => {
    const page = document.createElement('anth-theme-page') as AnthThemePage
    page.theme = THEMES.find((theme) => theme.id === 'system')
    page.tabs = []
    document.body.append(page)

    const tabs = await settle(page)

    expect(
      tabs.shadowRoot?.querySelectorAll('[role="tab"]'),
    ).toHaveLength(0)
  })
})
