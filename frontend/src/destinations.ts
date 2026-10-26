/**
 * Where the shell can navigate, in one place.
 *
 * The sidebar, the drawer and the tab strip all read this list, so a new
 * destination is declared once and appears in all three together.
 */

import { msg } from '@lit/localize'

export interface Destination {
  /** Stable id. It is the tab id, the route and the hash, so it never changes. */
  id: 'host' | 'settings'
  /** Full title, used by the sidebar and as the accessible name. */
  label: string
  /** Decorative glyph beside the label. */
  icon: string
}

/** Labels are built per call so a locale change is picked up by the next paint. */
export function destinations(): Destination[] {
  return [
    { id: 'host', label: msg('Host'), icon: '▤' },
    { id: 'settings', label: msg('Settings'), icon: '⚙' },
  ]
}

/** The destination a hash names, falling back to the first one. */
export function destinationFrom(hash: string): Destination {
  const id = hash.replace(/^#\/?/, '').trim()
  const all = destinations()
  return all.find((destination) => destination.id === id) ?? all[0]!
}

/** The hash for a destination, which is also what a link writes. */
export function hashFor(destination: Destination): string {
  return `#/${destination.id}`
}

/** The id for a hash, normalised to a destination that exists. */
export function idFromHash(hash: string): Destination['id'] {
  return destinationFrom(hash).id
}
