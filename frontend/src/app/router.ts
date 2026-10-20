/**
 * Hash routing for the panel shell.
 *
 * Hash routes keep the shell working when the API serves the SPA from the
 * embedded assets: no server rewrite is involved, and a deep link survives a
 * reload. The parser is pure so the shell can be tested without a DOM.
 */

export type Route =
  | { kind: 'home' }
  | { kind: 'panel'; panel: string }
  | { kind: 'unknown'; path: string }

/** Every route starts with `#/`; a missing or malformed hash is the home page. */
export function parseRoute(hash: string): Route {
  const raw = hash.startsWith('#') ? hash.slice(1) : hash
  const path = raw.replace(/^\/+/, '').replace(/\/+$/, '')

  if (path === '') return { kind: 'home' }

  const segments = path.split('/')
  if (segments[0] === 'panel' && segments.length === 2 && segments[1] !== '') {
    const panel = decodeSegment(segments[1])
    if (panel !== undefined) return { kind: 'panel', panel }
  }

  return { kind: 'unknown', path }
}

/**
 * Decodes one path segment, or gives up.
 *
 * A hand-edited or truncated link can contain a lone `%`, which makes
 * `decodeURIComponent` throw. Letting that escape would abort the shell's
 * constructor and leave a blank page, so a malformed segment is an unknown
 * route instead.
 */
function decodeSegment(segment: string): string | undefined {
  try {
    return decodeURIComponent(segment)
  } catch {
    return undefined
  }
}

/** Canonical hash for a route, so links and the address bar agree. */
export function routeHash(route: Route): string {
  switch (route.kind) {
    case 'home':
      return '#/'
    case 'panel':
      return `#/panel/${encodeURIComponent(route.panel)}`
    case 'unknown':
      return `#/${route.path}`
  }
}

/** URL for one panel, used by the card links. */
export function panelHash(panel: string): string {
  return routeHash({ kind: 'panel', panel })
}
