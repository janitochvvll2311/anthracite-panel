import { describe, expect, it } from 'vitest'

import { panelHash, parseRoute, routeHash, windowUrl } from './router'

describe('parseRoute', () => {
  it('treats an empty hash as the home page', () => {
    expect(parseRoute('')).toEqual({ kind: 'home' })
    expect(parseRoute('#')).toEqual({ kind: 'home' })
    expect(parseRoute('#/')).toEqual({ kind: 'home' })
  })

  it('parses a panel route and decodes its id', () => {
    expect(parseRoute('#/panel/processes')).toEqual({
      kind: 'panel',
      panel: 'processes',
    })
    expect(parseRoute('#/panel/a%20b')).toEqual({ kind: 'panel', panel: 'a b' })
  })

  it('tolerates missing and trailing slashes', () => {
    expect(parseRoute('panel/network')).toEqual({
      kind: 'panel',
      panel: 'network',
    })
    expect(parseRoute('#/panel/network/')).toEqual({
      kind: 'panel',
      panel: 'network',
    })
  })

  it('treats a malformed percent escape as an unknown path', () => {
    // A hand-edited link must not abort the shell's constructor.
    expect(parseRoute('#/panel/%')).toEqual({ kind: 'unknown', path: 'panel/%' })
    expect(parseRoute('#/panel/a%zz')).toEqual({
      kind: 'unknown',
      path: 'panel/a%zz',
    })
  })

  it('reports anything else as an unknown path', () => {
    expect(parseRoute('#/nope')).toEqual({ kind: 'unknown', path: 'nope' })
    expect(parseRoute('#/panel')).toEqual({ kind: 'unknown', path: 'panel' })
    expect(parseRoute('#/panel/a/b')).toEqual({
      kind: 'unknown',
      path: 'panel/a/b',
    })
  })
})

describe('routeHash', () => {
  it('round-trips every route kind', () => {
    for (const hash of ['#/', '#/panel/files', '#/nope']) {
      expect(routeHash(parseRoute(hash))).toBe(hash)
    }
  })

  it('encodes a panel id with reserved characters', () => {
    expect(panelHash('a b')).toBe('#/panel/a%20b')
    expect(parseRoute(panelHash('a b'))).toEqual({ kind: 'panel', panel: 'a b' })
  })
})

describe('window routes', () => {
  it('parses a standalone window route', () => {
    expect(parseRoute('#/window/processes')).toEqual({
      kind: 'window',
      panel: 'processes',
    })
  })

  it('round-trips a window route', () => {
    const route = { kind: 'window' as const, panel: 'host info' }
    expect(parseRoute(routeHash(route))).toEqual(route)
  })

  it('builds an absolute URL without the current hash', () => {
    expect(windowUrl('files', 'http://host:8080/')).toBe(
      'http://host:8080/#/window/files',
    )
  })
})
