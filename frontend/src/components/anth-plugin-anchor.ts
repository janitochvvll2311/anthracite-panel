import { LitElement, css, html, nothing } from 'lit'
import type { PropertyValues } from 'lit'
import { customElement, property, state } from 'lit/decorators.js'

import type {
  PluginClient,
  PluginUiAnchor,
} from '../lib/plugins/plugin-client'
import { loadAnchor } from '../lib/plugins/plugin-client'

/**
 * Loads one plugin UI anchor and appends the element it declares.
 *
 * `anth-plugin-slot` loads every anchor in a slot; a theme tab needs exactly one
 * anchor, with the label the shell already resolved. Keeping the loader here
 * means the tab content can be built synchronously while the module import runs
 * in the background, and the loaded element survives re-renders of the theme
 * page.
 */
@customElement('anth-plugin-anchor')
export class AnthPluginAnchor extends LitElement {
  @property({ attribute: false })
  plugin = ''

  @property({ attribute: false })
  anchor?: PluginUiAnchor

  @property({ attribute: false })
  client?: PluginClient

  @state()
  private error = ''

  private loadedKey = ''

  override connectedCallback(): void {
    super.connectedCallback()
    void this.load()
  }

  protected override updated(changed: PropertyValues<this>): void {
    if (
      changed.has('plugin') ||
      changed.has('anchor') ||
      changed.has('client')
    ) {
      void this.load()
    }
  }

  private async load(): Promise<void> {
    const plugin = this.plugin
    const anchor = this.anchor
    const client = this.client
    if (!plugin || !anchor || !client) return

    const key = `${plugin}|${anchor.module}|${anchor.tag}|${anchor.slot}`
    if (key === this.loadedKey) return
    this.loadedKey = key
    this.error = ''

    try {
      const element = await loadAnchor(client, plugin, anchor)
      if (this.loadedKey !== key) return
      this.replaceChildren(element)
    } catch (error) {
      if (this.loadedKey !== key) return
      this.loadedKey = ''
      this.error =
        error instanceof Error ? error.message : String(error)
      console.error(`[plugin] ${plugin} failed to load ${anchor.tag}:`, error)
    }
  }

  override render() {
    return html`
      <slot></slot>
      ${this.error
        ? html`<p class="error" role="alert">
            The plugin panel could not be loaded: ${this.error}
          </p>`
        : nothing}
    `
  }

  static override styles = css`
    :host {
      display: block;
      min-width: 0;
    }

    .error {
      margin: 0;
      padding: 0.6rem 0.75rem;
      color: var(--err, #f87171);
      border: 1px solid rgba(248, 113, 113, 0.45);
      border-radius: 8px;
      background: rgba(127, 29, 29, 0.25);
      font-size: 13px;
    }
  `
}

declare global {
  interface HTMLElementTagNameMap {
    'anth-plugin-anchor': AnthPluginAnchor
  }
}
