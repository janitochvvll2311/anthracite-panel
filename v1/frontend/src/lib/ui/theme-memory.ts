/**
 * The last panel opened in each theme.
 *
 * The route carries the panel, so this is only a hint for the sidebar: clicking
 * "Network" should return to Firewall if that is where the operator left off.
 * The tab strip itself is controlled by the route, never by this store, so a
 * stale value cannot make the visible tab disagree with the address bar.
 */

const PREFIX = 'anth.theme.'

/** The remembered panel id for a theme, or `undefined`. */
export function rememberedPanel(themeId: string): string | undefined {
  try {
    return globalThis.localStorage?.getItem(PREFIX + themeId) ?? undefined
  } catch {
    return undefined
  }
}

/** Records the panel the operator last opened in a theme. */
export function rememberPanel(themeId: string, panelId: string): void {
  try {
    globalThis.localStorage?.setItem(PREFIX + themeId, panelId)
  } catch {
    // A blocked storage API only costs the remembered tab.
  }
}
