import { afterEach, describe, expect, it, vi } from 'vitest'

import { rememberedPanel, rememberPanel } from './theme-memory'

afterEach(() => {
  localStorage.clear()
  vi.restoreAllMocks()
})

describe('theme memory', () => {
  it('remembers one panel per theme without sharing the key', () => {
    rememberPanel('system', 'processes')
    rememberPanel('network', 'firewall')

    expect(rememberedPanel('system')).toBe('processes')
    expect(rememberedPanel('network')).toBe('firewall')
    expect(rememberedPanel('storage')).toBeUndefined()
  })

  it('overwrites the previous panel for the same theme', () => {
    rememberPanel('system', 'host-info')
    rememberPanel('system', 'processes')

    expect(rememberedPanel('system')).toBe('processes')
  })

  it('survives a storage that refuses reads', () => {
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('blocked')
    })

    expect(rememberedPanel('system')).toBeUndefined()
  })

  it('survives a storage that refuses writes', () => {
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('blocked')
    })

    expect(() => rememberPanel('system', 'processes')).not.toThrow()
  })
})
