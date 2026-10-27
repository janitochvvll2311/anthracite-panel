/**
 * Where the shell can navigate, in one place.
 *
 * There are two levels: the sidebar sections (Home, Files, Settings) and, inside
 * a section that owns more than one panel, the tab strip. The sidebar and the
 * drawer render the sections; the tab strip renders the panels of the section
 * that is open. That keeps a destination declared once and out of the other
 * level's list.
 */

import { msg } from '@lit/localize'

export type SectionId = 'home' | 'files' | 'settings'
export type PanelId = 'general' | 'files'

export interface Section {
  /** Stable id. It is the route and the hash, so it never changes. */
  id: SectionId
  /** Title, used by the sidebar, the drawer and the document. */
  label: string
  /** Decorative glyph beside the label. */
  icon: string
}

export interface Panel {
  /** Stable id. It is the tab id and the route segment. */
  id: PanelId
  label: string
}

export interface Route {
  section: SectionId
  panel: PanelId
}

/** Labels are built per call so a locale change is picked up by the next paint. */
export function sections(): Section[] {
  return [
    { id: 'home', label: msg('Home'), icon: 'home' },
    { id: 'files', label: msg('Files'), icon: 'files' },
    { id: 'settings', label: msg('Settings'), icon: 'settings' },
  ]
}

/** The single panel the Files section owns. */
export function filesPanels(): Panel[] {
  return [{ id: 'files', label: msg('Files') }]
}

/** The single panel the Settings section owns. */
export function settingsPanels(): Panel[] {
  return [{ id: 'general', label: msg('General') }]
}

/** The route a hash names, falling back to Home. */
export function routeFrom(hash: string): Route {
  const parts = hash
    .replace(/^#\/?/, '')
    .split('/')
    .filter(Boolean)
  if (parts[0] === 'files') {
    return { section: 'files', panel: 'files' }
  }
  if (parts[0] === 'settings') {
    return { section: 'settings', panel: 'general' }
  }
  return { section: 'home', panel: 'general' }
}

/** The hash for a section. */
export function hashForSection(section: SectionId): string {
  switch (section) {
    case 'files':
      return '#/files'
    case 'settings':
      return '#/settings'
    default:
      return '#/'
  }
}

/** The hash for one panel. */
export function hashForPanel(panel: PanelId): string {
  return panel === 'files' ? '#/files' : '#/settings'
}
