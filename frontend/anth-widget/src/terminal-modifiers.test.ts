import { describe, expect, it } from 'vitest'

import { applyStickyModifiers } from './lib/terminal/modifiers'

describe('applyStickyModifiers', () => {
  it('leaves input alone without modifiers', () => {
    expect(applyStickyModifiers('c', false, false)).toBe('c')
  })

  it('maps Ctrl letters to control codes', () => {
    expect(applyStickyModifiers('c', true, false)).toBe('\x03')
    expect(applyStickyModifiers('C', true, false)).toBe('\x03')
    expect(applyStickyModifiers('d', true, false)).toBe('\x04')
    expect(applyStickyModifiers('z', true, false)).toBe('\x1a')
  })

  it('maps Ctrl symbols the way xterm does', () => {
    expect(applyStickyModifiers('[', true, false)).toBe('\x1b')
    expect(applyStickyModifiers('_', true, false)).toBe('\x1f')
    expect(applyStickyModifiers('?', true, false)).toBe('\x7f')
  })

  it('prefixes Alt with ESC', () => {
    expect(applyStickyModifiers('c', false, true)).toBe('\x1bc')
  })

  it('ignores control characters and multi-character input', () => {
    expect(applyStickyModifiers('\r', true, false)).toBe('\r')
    expect(applyStickyModifiers('\x7f', true, false)).toBe('\x7f')
    expect(applyStickyModifiers('echo', true, false)).toBe('echo')
  })
})
