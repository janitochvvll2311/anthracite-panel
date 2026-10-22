import { LitElement, css, html, nothing } from 'lit'
import { customElement, property, state } from 'lit/decorators.js'

import type { PluginsCatalogue } from '../lib/plugins/plugin-client'
import { HttpPluginClient, loadSlot } from '../lib/plugins/plugin-client'

/**
 * Mounts whatever plugins declared for one slot.
 *
 * The slot name is the contract: the shell renders one of these wherever a
 * plugin is allowed to appear, and this element asks the loader for the
 * elements that belong there. The shell therefore contains no plugin tags and
 * no plugin imports; a plugin is data until it loads.
 *
 * An element per anchor is created once and kept. Re-rendering on a catalogue
 * change would otherwise reload every module, and a module that registers a
 * custom element cannot be registered twice in the same realm.
 */
@customElement('anth-plugin-slot')
export class AnthPluginSlot extends LitElement {
  static override styles = css`
    :host {
      display: block;
    }

    :host([slot-name='dashboard.widget']) {
      display: contents;
    }

    .empty {
      color: var(--muted, #8b9bb4);
      font-size: 0.85rem;
    }
  `

  /** Which slot this element fills, e.g. `dashboard.widget`. */
  @property({ reflect: true, attribute: 'slot-name' })
  slotName = ''

  /** The catalogue, owned by the shell so every slot reads it once. */
  @property({ attribute: false })
  catalogue?: PluginsCatalogue

  /** Injected in tests; the default talks to `/api/plugins`. */
  @property({ attribute: false })
  client?: HttpPluginClient

  @state()
  private mounted = 0

  private current?: PluginsCatalogue

  override connectedCallback(): void {
    super.connectedCallback()
    void this.refresh()
  }

  protected override updated(): void {
    if (this.catalogue !== this.current) void this.refresh()
  }

  /**
   * Loads the slot's modules and replaces the light DOM with the results.
   *
   * The host is light DOM on purpose: a page slot may need to sit inside the
   * panel's layout, and a plugin's element styles itself. Keeping the elements
   * in the light DOM also lets the tab panel's mutation observer see a plugin
   * tab the same way it sees a built-in one.
   */
  private async refresh(): Promise<void> {
    this.current = this.catalogue
    const client = this.client ?? new HttpPluginClient()
    const elements = await loadSlot(client, this.catalogue, this.slotName)
    this.replaceChildren(...elements)
    this.mounted = elements.length
  }

  override render() {
    // The children are managed imperatively, so the shadow root only needs the
    // empty hint, and only when it is true.
    return this.mounted === 0 && this.catalogue?.enabled
      ? html`<p class="empty">${this.slotName}</p>`
      : nothing
  }
}

declare global {
  interface HTMLElementTagNameMap {
    'anth-plugin-slot': AnthPluginSlot
  }
}
