/**
 * Where the shell can navigate, in one place.
 *
 * There are two levels: the sidebar sections (Home, Settings) and, inside
 * Settings, the panels the section owns. The sidebar and the drawer render the
 * sections; the tab strip renders the panels of the section that is open. That
 * keeps a destination declared once and out of the other level's list.
 */

import { msg } from '@lit/localize'

export type SectionId = 'home' | 'settings'
export type SettingsPanelId = 'general'

export interface Section {
  /** Stable id. It is the route and the hash, so it never changes. */
  id: SectionId
  /** Title, used by the sidebar, the drawer and the document. */
  label: string
  /** Decorative glyph beside the label. */
  icon: string
}

export interface SettingsPanel {
  /** Stable id. It is the tab id and the route segment. */
  id: SettingsPanelId
  label: string
}

export interface Route {
  section: SectionId
  /** Only meaningful when `section` is `settings`. */
  panel: SettingsPanelId
}

/** Labels are built per call so a locale change is picked up by the next paint. */
export function sections(): Section[] {
  return [
    { id: 'home', label: msg('Home'), icon: '▤' },
    { id: 'settings', label: msg('Settings'), icon: '⚙' },
  ]
}

/** The panels that live under the Settings section. */
export function settingsPanels(): SettingsPanel[] {
  return [{ id: 'general', label: msg('General') }]
}

/** The route a hash names, falling back to Home. */
export function routeFrom(hash: string): Route {
  const parts = hash
    .replace(/^#\/?/, '')
    .split('/')
    .filter(Boolean)
  if (parts[0] === 'settings') {
    return { section: 'settings', panel: 'general' }
  }
  return { section: 'home', panel: 'general' }
}

/** The hash for a section. Settings defaults to its only panel. */
export function hashForSection(section: SectionId): string {
  return section === 'settings' ? '#/settings' : '#/'
}

/** The hash for one panel inside Settings. */
export function hashForPanel(panel: SettingsPanelId): string {
  return panel === 'general' ? '#/settings' : `#/settings/${panel}`
}
