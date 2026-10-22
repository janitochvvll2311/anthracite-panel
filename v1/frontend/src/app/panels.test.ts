import { describe, expect, it } from 'vitest'

import { PANELS, THEMES, panelsIn, themeForPanel } from './panels'

describe('panel themes', () => {
  it('assigns every panel to exactly one theme', () => {
    const grouped = THEMES.flatMap((theme) =>
      panelsIn(theme.id).map((panel) => panel.id),
    )

    expect(grouped.sort()).toEqual(PANELS.map((panel) => panel.id).sort())
    expect(new Set(grouped).size).toBe(grouped.length)
  })

  it('gives every theme at least one panel', () => {
    for (const theme of THEMES) {
      expect(panelsIn(theme.id).length).toBeGreaterThan(0)
    }
  })

  it('resolves the theme that owns a panel', () => {
    expect(themeForPanel('host-info')?.id).toBe('system')
    expect(themeForPanel('firewall')?.id).toBe('network')
    expect(themeForPanel('terminal')?.id).toBe('extensions')
    expect(themeForPanel('does-not-exist')).toBeUndefined()
  })

  it('keeps the tab order stable inside a theme', () => {
    expect(panelsIn('system').map((panel) => panel.id)).toEqual([
      'host-info',
      'processes',
    ])
    expect(panelsIn('storage').map((panel) => panel.id)).toEqual([
      'files',
      'volumes',
    ])
  })
})
