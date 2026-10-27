import { msg } from '@lit/localize'

import { AnthFileTabs, FILE_TABS_EMPTY_EVENT } from './file-tabs'
import type { OpenFileEditor } from './file-tabs'
import { WINDOW_CLOSE_EVENT } from './window'
import type { AnthWindow } from './window'

let editorWindow: AnthWindow | undefined
let editorHost: AnthFileTabs | undefined

/**
 * The Files section has one editor window for the whole page. Opening a file
 * adds a tab to it; the window lives on `document.body`, so navigating the
 * panel cannot close it or lose its tabs.
 */
export function openFileEditor(options: OpenFileEditor): AnthWindow {
  if (!editorWindow?.isConnected || !editorHost) {
    const host = document.createElement('anth-file-tabs') as AnthFileTabs
    host.client = options.client
    const window = document.createElement('anth-window') as AnthWindow
    window.label = msg('Editor')
    window.append(host)
    host.addEventListener(FILE_TABS_EMPTY_EVENT, () => window.close())
    window.addEventListener(WINDOW_CLOSE_EVENT, () => {
      window.remove()
      if (editorWindow === window) {
        editorWindow = undefined
        editorHost = undefined
      }
    })
    document.body.append(window)
    editorWindow = window
    editorHost = host
  }

  const window = editorWindow
  const host = editorHost
  if (!window || !host) {
    throw new Error('the editor window could not be created')
  }
  window.restore()
  window.open = true
  host.open(options)
  host.focusActive()
  return window
}
