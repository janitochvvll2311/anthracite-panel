import { msg } from '@lit/localize'

import { TERMINAL_TABS_EMPTY_EVENT } from './terminal-tabs'
import type { AnthTerminalTabs } from './terminal-tabs'
import { WINDOW_CLOSE_EVENT } from './window'
import type { AnthWindow } from './window'

let terminalWindow: AnthWindow | undefined
let terminalHost: AnthTerminalTabs | undefined

/**
 * Opens the one global terminal window, or focuses it when it exists.
 *
 * It lives on `document.body`, so navigating to Files or Settings does not
 * close the WebSocket. Closing the last tab closes the window and every PTY it
 * owns.
 */
export async function openTerminalWindow(): Promise<void> {
  if (!terminalWindow?.isConnected || !terminalHost) {
    const host = document.createElement('anth-terminal-tabs') as AnthTerminalTabs
    const window = document.createElement('anth-window') as AnthWindow
    window.label = msg('Terminal')
    window.nonmodal = true
    window.style.setProperty('--anth-window-width', 'min(100%, 52rem)')
    window.append(host)
    host.addEventListener(TERMINAL_TABS_EMPTY_EVENT, () => window.close())
    window.addEventListener(WINDOW_CLOSE_EVENT, () => {
      window.remove()
      if (terminalWindow === window) {
        terminalWindow = undefined
        terminalHost = undefined
      }
    })
    document.body.append(window)
    terminalWindow = window
    terminalHost = host
  }

  const window = terminalWindow
  const host = terminalHost
  if (!window || !host) {
    throw new Error('the terminal window could not be created')
  }
  window.restore()
  window.open = true
  await host.updateComplete
  if (host.sessionCount === 0) {
    await host.openSession()
  }
  host.focusActive()
}

/** Closes the global terminal window and every session it owns. */
export function closeTerminalWindow(): void {
  terminalWindow?.close()
}

/** Test hook: the current singleton, if any. */
export function currentTerminalWindow(): AnthWindow | undefined {
  return terminalWindow
}
