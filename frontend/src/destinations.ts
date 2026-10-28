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

export type SectionId =
  | 'home'
  | 'files'
  | 'terminal'
  | 'processes'
  | 'network'
  | 'storage'
  | 'accounts'
  | 'settings'
export type PanelId =
  | 'general'
  | 'files'
  | 'terminal'
  | 'interfaces'
  | 'firewall'
  | 'volumes'
  | 'partitions'
  | 'users'
  | 'groups'
  | 'plugins'

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
    { id: 'terminal', label: msg('Terminal'), icon: 'terminal' },
    { id: 'processes', label: msg('Processes'), icon: 'process' },
    { id: 'network', label: msg('Network'), icon: 'network' },
    { id: 'storage', label: msg('Storage'), icon: 'storage' },
    { id: 'accounts', label: msg('Accounts'), icon: 'accounts' },
    { id: 'settings', label: msg('Settings'), icon: 'settings' },
  ]
}

/** The single panel the Files section owns. */
export function filesPanels(): Panel[] {
  return [{ id: 'files', label: msg('Files') }]
}

/** The panels the Network section owns. */
export function networkPanels(): Panel[] {
  return [
    { id: 'interfaces', label: msg('Interfaces') },
    { id: 'firewall', label: msg('Firewall') },
  ]
}

/** The panels the Storage section owns. */
export function storagePanels(): Panel[] {
  return [
    { id: 'volumes', label: msg('Volumes') },
    { id: 'partitions', label: msg('Partitions') },
  ]
}

/** The panels the Accounts section owns. */
export function accountsPanels(): Panel[] {
  return [
    { id: 'users', label: msg('Users') },
    { id: 'groups', label: msg('Groups') },
  ]
}

/** The panels the Settings section owns. */
export function settingsPanels(): Panel[] {
  return [
    { id: 'general', label: msg('General') },
    { id: 'plugins', label: msg('Plugins') },
  ]
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
  if (parts[0] === 'terminal') {
    return { section: 'terminal', panel: 'terminal' }
  }
  if (parts[0] === 'processes') {
    return { section: 'processes', panel: 'general' }
  }
  if (parts[0] === 'network') {
    return {
      section: 'network',
      panel: parts[1] === 'firewall' ? 'firewall' : 'interfaces',
    }
  }
  if (parts[0] === 'storage') {
    return {
      section: 'storage',
      panel: parts[1] === 'partitions' ? 'partitions' : 'volumes',
    }
  }
  if (parts[0] === 'accounts') {
    return {
      section: 'accounts',
      panel: parts[1] === 'groups' ? 'groups' : 'users',
    }
  }
  if (parts[0] === 'settings') {
    return {
      section: 'settings',
      panel: parts[1] === 'plugins' ? 'plugins' : 'general',
    }
  }
  return { section: 'home', panel: 'general' }
}

/** The hash for a section. */
export function hashForSection(section: SectionId): string {
  switch (section) {
    case 'files':
      return '#/files'
    case 'terminal':
      return '#/terminal'
    case 'processes':
      return '#/processes'
    case 'network':
      return '#/network'
    case 'storage':
      return '#/storage'
    case 'accounts':
      return '#/accounts'
    case 'settings':
      return '#/settings'
    default:
      return '#/'
  }
}

/** The hash for one panel. */
export function hashForPanel(panel: PanelId): string {
  if (panel === 'files') return '#/files'
  if (panel === 'terminal') return '#/terminal'
  if (panel === 'interfaces') return '#/network'
  if (panel === 'firewall') return '#/network/firewall'
  if (panel === 'volumes') return '#/storage'
  if (panel === 'partitions') return '#/storage/partitions'
  if (panel === 'users') return '#/accounts'
  if (panel === 'groups') return '#/accounts/groups'
  if (panel === 'plugins') return '#/settings/plugins'
  return '#/settings'
}
