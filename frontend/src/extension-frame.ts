/**
 * `anth-extension-frame`: one declared extension UI module in a sandboxed iframe.
 *
 * The iframe has an opaque origin (`sandbox="allow-scripts"`, no
 * `allow-same-origin`), so the module sees no panel DOM, cookies or storage.
 * It talks to this element only through `postMessage`; every `call` is checked
 * against the extension's frozen anchors before it becomes a same-origin fetch.
 */

import { LitElement, css, html, nothing } from 'lit'
import { customElement, property, state } from 'lit/decorators.js'
import { localized, msg } from '@lit/localize'

import { currentLocale } from './i18n'
import type { ExtensionUiSlot, ExtensionView } from './extensions-client'

interface BridgeCall {
  anthExtension?: boolean
  type?: string
  requestId?: number
  method?: string
  path?: string
  body?: unknown
  height?: number
  message?: string
}

const THEME_TOKENS = [
  'text',
  'muted',
  'panel',
  'panel-2',
  'panel-border',
  'accent',
  'ok',
  'warn',
  'err',
  'mono',
  'sans',
]

@customElement('anth-extension-frame')
@localized()
export class AnthExtensionFrame extends LitElement {
  @property({ attribute: false })
  extension?: ExtensionView

  @property({ attribute: false })
  uiSlot?: ExtensionUiSlot

  @state()
  private error = ''

  @state()
  private height = 160

  override connectedCallback(): void {
    super.connectedCallback()
    globalThis.addEventListener('message', this.#onMessage)
  }

  override disconnectedCallback(): void {
    globalThis.removeEventListener('message', this.#onMessage)
    super.disconnectedCallback()
  }

  #frame(): HTMLIFrameElement | null {
    return this.shadowRoot?.querySelector('iframe') ?? null
  }

  #source(): string {
    const extension = this.extension
    const slot = this.uiSlot
    if (!extension || !slot) return 'about:blank'
    const module = slot.module.replace(/^web\//, '')
    const path = module.split('/').map(encodeURIComponent).join('/')
    return `/e/${encodeURIComponent(extension.id)}/${path}`
  }

  #theme(): Record<string, string> {
    const style = getComputedStyle(document.documentElement)
    const theme: Record<string, string> = {}
    for (const token of THEME_TOKENS) {
      const value = style.getPropertyValue(`--${token}`).trim()
      if (value !== '') theme[token] = value
    }
    return theme
  }

  #sendInit(): void {
    const extension = this.extension
    const slot = this.uiSlot
    const frame = this.#frame()
    if (!extension || !slot || !frame?.contentWindow) return
    frame.contentWindow.postMessage(
      {
        anthExtensionHost: true,
        type: 'init',
        snapshot: { extension, slot },
        locale: currentLocale(),
        theme: this.#theme(),
      },
      '*',
    )
  }

  #reply(requestId: number, ok: boolean, payload: Record<string, unknown>): void {
    const frame = this.#frame()
    frame?.contentWindow?.postMessage(
      { anthExtensionHost: true, type: 'result', requestId, ok, ...payload },
      '*',
    )
  }

  async #call(data: BridgeCall): Promise<void> {
    const requestId = data.requestId
    const extension = this.extension
    if (typeof requestId !== 'number' || !extension) return
    const method = (data.method ?? 'POST').toUpperCase()
    const path = data.path ?? ''
    const declared = extension.api.some(
      (anchor) => anchor.method.toUpperCase() === method && anchor.path === path,
    )
    if (!declared || !path.startsWith('/')) {
      this.#reply(requestId, false, {
        message: `the extension does not declare ${method} ${path}`,
      })
      return
    }
    const init: RequestInit = {
      method,
      headers: { 'Content-Type': 'application/json' },
      credentials: 'same-origin',
    }
    if (method !== 'GET' && data.body !== undefined && data.body !== null) {
      init.body =
        typeof data.body === 'string' ? data.body : JSON.stringify(data.body)
    }
    if (method === 'GET') delete init.headers
    try {
      const response = await fetch(
        `/api/extensions/${encodeURIComponent(extension.id)}${path}`,
        init,
      )
      const text = await response.text()
      let body: unknown = text
      try {
        body = text === '' ? null : JSON.parse(text)
      } catch {
        // A plain-text anchor reply is valid; keep the text.
      }
      this.#reply(requestId, true, { status: response.status, body })
    } catch (failure) {
      this.#reply(requestId, false, { message: String(failure) })
    }
  }

  readonly #onMessage = (event: MessageEvent): void => {
    const frame = this.#frame()
    if (!frame || event.source !== frame.contentWindow) return
    if (event.origin !== 'null' && event.origin !== '') return
    const data = event.data as BridgeCall | undefined
    if (!data || data.anthExtension !== true) return
    switch (data.type) {
      case 'ready':
        this.#sendInit()
        break
      case 'mounted':
        this.error = ''
        break
      case 'error':
        this.error = data.message ?? msg('The extension widget failed.')
        break
      case 'resize':
        if (typeof data.height === 'number' && Number.isFinite(data.height)) {
          this.height = Math.max(80, Math.min(1200, Math.round(data.height) + 8))
        }
        break
      case 'call':
        void this.#call(data)
        break
    }
  }

  override render() {
    const extension = this.extension
    const slot = this.uiSlot
    if (!extension || !slot) return nothing
    return html`
      <section class="widget" part="widget">
        <header>
          <span class="slot">${slot.label ?? slot.slot}</span>
          <span class="extension">${extension.name}</span>
        </header>
        <iframe
          title=${slot.label ?? `${extension.name} widget`}
          sandbox="allow-scripts"
          referrerpolicy="no-referrer"
          src=${this.#source()}
          style=${`height: ${this.height}px`}
          @load=${() => {
            this.error = ''
          }}
        ></iframe>
        ${this.error
          ? html`<p class="error" role="alert">${this.error}</p>`
          : nothing}
      </section>
    `
  }

  static styles = css`
    :host { display: block; }
    .widget {
      display: flex;
      flex-direction: column;
      gap: 0.35rem;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 9px;
      background: var(--panel-2, #0b121b);
      overflow: hidden;
    }
    header {
      display: flex;
      justify-content: space-between;
      gap: 0.5rem;
      padding: 0.35rem 0.55rem;
      border-bottom: 1px solid var(--panel-border, #223041);
      color: var(--muted, #8b9bad);
      font: 10px/1.4 var(--mono, monospace);
      letter-spacing: 0.05em;
      text-transform: uppercase;
    }
    .extension { color: var(--text, #dbe6f2); }
    iframe {
      display: block;
      width: 100%;
      min-height: 80px;
      border: 0;
      background: transparent;
      color-scheme: normal;
    }
    .error {
      margin: 0;
      padding: 0.45rem 0.55rem;
      border-top: 1px solid rgba(248, 113, 113, 0.35);
      color: var(--err, #f87171);
      font: 12px/1.4 var(--mono, monospace);
      overflow-wrap: anywhere;
    }
  `
}

declare global {
  interface HTMLElementTagNameMap {
    'anth-extension-frame': AnthExtensionFrame
  }
}
