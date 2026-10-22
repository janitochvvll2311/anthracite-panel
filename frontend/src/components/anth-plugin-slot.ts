import { LitElement, css, html } from 'lit'
import { customElement, property } from 'lit/decorators.js'

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
    /* The host is a layout pass-through: the anchored element is laid out by
       the surrounding grid, exactly as if it had been placed there directly. */
    :host {
      display: contents;
    }

    /* display: contents outranks the user-agent [hidden] rule, so the empty
       state needs its own rule; without it the attribute is inert and an empty
       slot still occupies a grid track. */
    :host([hidden]) {
      display: none;
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
    // `display: contents` on the host means the section's grid lays out the
    // anchored elements directly; hiding the host when it has none is what
    // keeps an empty dashboard from reserving a grid row.
    this.hidden = elements.length === 0
  }

  /**
   * The anchored elements live in the light DOM, so the shadow root has to
   * project them with `<slot>`; without it they are parsed, appended and never
   * painted.
   *
   * There is no placeholder text for an empty slot. Nothing installed is the
   * normal state, and rendering the slot's own name would put an internal
   * identifier on the dashboard; `hidden` keeps the empty host out of layout
   * instead.
   */
  override render() {
    return html`<slot></slot>`
  }
}

declare global {
  interface HTMLElementTagNameMap {
    'anth-plugin-slot': AnthPluginSlot
  }
}
