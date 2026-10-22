import { beforeEach, describe, expect, it, vi } from 'vitest'

import {
  MIN_HEIGHT,
  MIN_WIDTH,
  TITLE_BAR_HEIGHT,
  clampRect,
  dragRect,
  fitToViewport,
  initialRect,
  loadWindows,
  nextZ,
  resizeRect,
  saveWindows,
  type WindowState,
} from './windows'

const viewport = { width: 1000, height: 800 }

function state(patch: Partial<WindowState> = {}): WindowState {
  return {
    id: 'processes',
    rect: { x: 100, y: 100, width: 600, height: 400 },
    minimized: false,
    z: 1,
    ...patch,
  }
}

describe('initialRect', () => {
  it('fits inside a small viewport', () => {
    const rect = initialRect(0, { width: 400, height: 300 })
    expect(rect.width).toBeGreaterThanOrEqual(MIN_WIDTH)
    expect(rect.x).toBeGreaterThanOrEqual(0)
    expect(rect.x + rect.width).toBeLessThanOrEqual(400)
  })

  it('cascades so two windows do not overlap exactly', () => {
    const first = initialRect(0, viewport)
    const second = initialRect(1, viewport)
    expect(second.x).toBeGreaterThan(first.x)
    expect(second.y).toBeGreaterThan(first.y)
  })

  it('wraps the cascade rather than marching off-screen', () => {
    expect(initialRect(5, viewport)).toEqual(initialRect(0, viewport))
  })
})

describe('fitToViewport', () => {
  it('pulls a window back when the viewport shrinks', () => {
    const rect = fitToViewport(
      { x: 900, y: 700, width: 600, height: 400 },
      { width: 800, height: 600 },
    )
    expect(rect.x).toBe(200)
    expect(rect.y).toBe(600 - TITLE_BAR_HEIGHT)
    expect(rect.width).toBe(600)
  })

  it('shrinks a window that is wider than the viewport', () => {
    const rect = fitToViewport(
      { x: 0, y: 0, width: 2000, height: 400 },
      viewport,
    )
    expect(rect.width).toBe(1000)
    expect(rect.x).toBe(0)
  })
})

describe('clampRect', () => {
  it('leaves the origin alone and cuts the size instead', () => {
    const rect = clampRect(
      { x: 100, y: 100, width: 5000, height: 5000 },
      viewport,
    )
    expect(rect.x).toBe(100)
    expect(rect.width).toBe(900)
    expect(rect.height).toBe(700)
  })

  it('never goes below the minimum size', () => {
    const rect = clampRect(
      { x: 0, y: 0, width: 10, height: 10 },
      viewport,
    )
    expect(rect.width).toBe(MIN_WIDTH)
    expect(rect.height).toBe(MIN_HEIGHT)
  })

  it('never leaves the origin beyond reach', () => {
    const rect = clampRect(
      { x: 5000, y: 5000, width: 600, height: 400 },
      viewport,
    )
    expect(rect.x).toBe(viewport.width - MIN_WIDTH)
    expect(rect.y).toBe(viewport.height - TITLE_BAR_HEIGHT)
  })

  it('keeps a negative origin off-screen', () => {
    const rect = clampRect(
      { x: -400, y: -400, width: 600, height: 400 },
      viewport,
    )
    expect(rect.x).toBe(0)
    expect(rect.y).toBe(0)
  })
})

describe('dragRect', () => {
  it('applies the delta to the origin', () => {
    const moved = dragRect(
      { x: 100, y: 100, width: 600, height: 400 },
      { dx: 50, dy: -30 },
      viewport,
    )
    expect(moved.x).toBe(150)
    expect(moved.y).toBe(70)
  })

  it('stops at the edge instead of following the pointer away', () => {
    const moved = dragRect(
      { x: 0, y: 0, width: 600, height: 400 },
      { dx: -500, dy: -500 },
      viewport,
    )
    expect(moved.x).toBe(0)
    expect(moved.y).toBe(0)
  })

  it('leaves the title bar on screen at the bottom', () => {
    const moved = dragRect(
      { x: 100, y: 100, width: 600, height: 400 },
      { dx: 0, dy: 5000 },
      viewport,
    )
    expect(moved.y).toBe(viewport.height - TITLE_BAR_HEIGHT)
  })
})

describe('resizeRect', () => {
  it('grows from the bottom-right corner', () => {
    const resized = resizeRect(
      { x: 100, y: 100, width: 400, height: 300 },
      { dx: 100, dy: 50 },
      viewport,
    )
    expect(resized.width).toBe(500)
    expect(resized.height).toBe(350)
    expect(resized.x).toBe(100)
  })

  it('refuses to collapse the window', () => {
    const resized = resizeRect(
      { x: 100, y: 100, width: 400, height: 300 },
      { dx: -5000, dy: -5000 },
      viewport,
    )
    expect(resized.width).toBe(MIN_WIDTH)
    expect(resized.height).toBe(MIN_HEIGHT)
  })

  it('stops growing at the viewport edge', () => {
    const resized = resizeRect(
      { x: 100, y: 100, width: 400, height: 300 },
      { dx: 5000, dy: 5000 },
      viewport,
    )
    expect(resized.width).toBe(900)
    expect(resized.height).toBe(700)
  })
})

describe('nextZ', () => {
  it('starts at one', () => {
    expect(nextZ([])).toBe(1)
  })

  it('is above every existing window', () => {
    expect(nextZ([state({ z: 4 }), state({ id: 'files', z: 9 })])).toBe(10)
  })
})

describe('persistence', () => {
  beforeEach(() => {
    localStorage.clear()
  })

  it('round-trips a layout', () => {
    const states = [state(), state({ id: 'files', z: 2 })]
    saveWindows('panel', states)
    expect(loadWindows('panel')).toEqual(states)
  })

  it('returns an empty layout when nothing was stored', () => {
    expect(loadWindows('missing')).toEqual([])
  })

  it('discards malformed entries instead of throwing', () => {
    localStorage.setItem(
      'anth.windows.panel',
      JSON.stringify([state(), { id: 'broken' }, null, 42]),
    )
    const loaded = loadWindows('panel')
    expect(loaded).toHaveLength(1)
    expect(loaded[0].id).toBe('processes')
  })

  it('survives a value that is not JSON at all', () => {
    localStorage.setItem('anth.windows.panel', '{oops')
    expect(loadWindows('panel')).toEqual([])
  })

  it('survives a storage that refuses to write', () => {
    const spy = vi
      .spyOn(Storage.prototype, 'setItem')
      .mockImplementation(() => {
        throw new Error('quota')
      })
    expect(() => saveWindows('panel', [state()])).not.toThrow()
    spy.mockRestore()
  })
})
