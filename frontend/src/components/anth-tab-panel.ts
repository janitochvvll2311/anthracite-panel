import { LitElement, css, html, nothing } from 'lit'
import type { PropertyValues } from 'lit'
import { customElement, property, state } from 'lit/decorators.js'

import { ANTH_TAB_SHOW_EVENT } from '../lib/ui/tab-visibility'

/**
 * Tabs that keep a hidden tab's state without keeping its DOM.
 *
 * The usual `hidden` attribute keeps every tab rendered: a background tab keeps
 * its SSE stream open, its WebSocket alive and its table re-rendering on every
 * frame. This component detaches the hidden tab instead. A detached custom
 * element keeps its properties, its shadow root and its internal state across
 * `remove()`/`append()`, so the tab resumes where it left off while it cannot
 * render.
 *
 * What it does and does not save:
 *
 * - Saved: connections, timers and rendering. A parked tab receives
 *   `disconnectedCallback`, so its SSE stream and WebSocket close and it stops
 *   re-rendering. A hidden process table no longer walks `/proc`.
 * - Not saved: the parsed subtree. Every declared tab is built by the HTML
 *   parser whether or not it is shown, so detaching an unvisited tab would free
 *   nothing while making the first visit slower. All tabs stay in `elements`;
 *   only the visible one is in the document.
 * - A tab that fetches on `connectedCallback` does so once, on first show, and
 *   not again while it stays parked.
 *
 * Usage:
 *
 * ```html
 * <anth-tab-panel active="users" storage-key="accounts">
 *   <section data-tab="users" label="Users">…</section>
 *   <section data-tab="groups" label="Groups">…</section>
 * </anth-tab-panel>
 * ```
 */

export interface TabDefinition {
  id: string
  label: string
  /** Set when the tab cannot be shown; the text becomes the tooltip. */
  disabled?: string
}

export const ANTH_TAB_CHANGE_EVENT = 'anth-tab-change'

export { ANTH_TAB_SHOW_EVENT }

const STORAGE_PREFIX = 'anth.tabs.'

@customElement('anth-tab-panel')
export class AnthTabPanel extends LitElement {
  /** Id of the visible tab. An empty value selects the first enabled tab. */
  @property({ reflect: true })
  active = ''

  /**
   * Remembers the chosen tab in `localStorage` under this key. Omitted, the
   * selection lives only as long as the element.
   *
   * The stored choice wins over an `active` written in the markup, which is
   * what makes a reload restore the tab the user last picked. `active` remains
   * the fallback for a first visit, and assigning the property after the
   * element is connected still selects a tab directly.
   */
  @property({ attribute: 'storage-key' })
  storageKey = ''

  @state()
  private tabs: TabDefinition[] = []

  /**
   * Every tab ever seen, in declaration order, whether it is in the document or
   * parked.
   *
   * The light DOM is the source of truth, but only mutation records can be read
   * from it: this component removes elements from its own slot, so a query at
   * an arbitrary time cannot tell a parked tab from one that never existed. The
   * observer below watches for *added* nodes and ignores the removals it causes
   * itself.
   */
  private elements = new Map<string, HTMLElement>()

  private order: string[] = []

  private observer?: MutationObserver

  override connectedCallback(): void {
    super.connectedCallback()

    // The stored choice wins over both the markup default and the first-tab
    // fallback in `syncTabs`; an id that no longer exists is dropped there.
    if (this.storageKey) {
      const stored = readStored(this.storageKey)
      if (stored) this.active = stored
    }

    this.observer = new MutationObserver((records) => {
      // `label` and `data-disabled` are read when the strip is built, so a tab
      // that changes them has to be rebuilt; a plain childList mutation only
      // needs the registry check.
      const metadata = records.some(
        (record) => record.type === 'attributes',
      )
      this.syncTabs(metadata)
    })
    this.observer.observe(this, { childList: true })
    // Re-observing is not `registerTabs`'s job: that method skips every id it
    // already knows, so a panel that is moved or re-appended would otherwise
    // end up with an observer that watches nothing. Watching every known tab
    // here covers both the first connection and a reconnection.
    for (const element of this.elements.values()) this.observeTab(element)
    this.syncTabs()
  }

  override disconnectedCallback(): void {
    this.observer?.disconnect()
    this.observer = undefined
    super.disconnectedCallback()
  }

  /**
   * Registers tabs that appear for the first time.
   *
   * Parking and restoring also mutate the light DOM, so an id already in the
   * registry is left where it is. Only a genuinely new child changes the strip,
   * which is what keeps a dynamic tab list from re-rendering on every switch.
   */
  private registerTabs(): boolean {
    let added = false
    for (const element of this.children) {
      if (!(element instanceof HTMLElement)) continue
      // A child without `data-tab` and without `id` cannot be addressed later,
      // so it is skipped rather than given a positional id that changes when
      // another tab is added ahead of it.
      const id = this.identify(element)
      if (!id) continue
      if (this.elements.has(id)) {
        // Re-added under the same id, e.g. moved or toggled: refresh the entry.
        this.elements.set(id, element)
        continue
      }
      this.elements.set(id, element)
      this.order.push(id)
      this.observeTab(element)
      added = true
    }
    return added
  }

  /**
   * Watches one tab's metadata.
   *
   * A parked tab is detached, and an observer on `this` only watches
   * descendants, so each element needs its own observer entry for the change
   * that re-enables or relabels a hidden tab to arrive.
   */
  private observeTab(element: HTMLElement): void {
    this.observer?.observe(element, {
      attributes: true,
      attributeFilter: ['label', 'data-label', 'data-disabled'],
    })
  }

  /** The declared id. A tab without one is not addressable and is ignored. */
  private identify(element: HTMLElement): string {
    return element.dataset.tab ?? element.id ?? ''
  }

  private syncTabs(metadataChanged = false): void {
    const added = this.registerTabs()
    if (!added && this.tabs.length > 0 && !metadataChanged) return

    this.tabs = this.order.map((id) => {
      const element = this.elements.get(id)!
      return {
        id,
        label: element.getAttribute('label') ?? element.dataset.label ?? id,
        disabled: element.dataset.disabled,
      }
    })

    const usable = this.tabs.filter((tab) => !tab.disabled)
    if (!usable.some((tab) => tab.id === this.active)) {
      this.active = usable[0]?.id ?? ''
    }
    this.applyVisibility()
  }

  /**
   * Puts the active tab in the document and parks every other.
   *
   * Parking is `remove()`, not `display: none`, and the element stays in
   * `elements` so `applyVisibility` can bring it back.
   */
  private applyVisibility(): void {
    if (this.elements.size === 0) return

    for (const [id, element] of this.elements) {
      const visible = id === this.active
      if (visible && !element.isConnected) {
        this.append(element)
        // Restoring changes no property, so a streaming child that only
        // starts from `willUpdate` would stay closed. This is the re-show
        // signal it listens for; see `lib/ui/tab-visibility`.
        element.dispatchEvent(
          new CustomEvent(ANTH_TAB_SHOW_EVENT, { bubbles: false }),
        )
      } else if (!visible && element.isConnected) {
        element.remove()
      }
    }
  }

  /**
   * The element registered for `id`, whether it is mounted or parked.
   *
   * A parked tab is out of the document, so `querySelector` cannot reach it.
   * A host that owns tab content and needs to change a hidden tab — enabling
   * it, updating its label, pushing data into it — goes through here.
   */
  tab(id: string): HTMLElement | undefined {
    return this.elements.get(id)
  }

  private select(id: string, focus = false): void {
    const tab = this.tabs.find((candidate) => candidate.id === id)
    if (!tab || tab.disabled || tab.id === this.active) return

    this.active = tab.id
    if (this.storageKey) writeStored(this.storageKey, tab.id)
    this.applyVisibility()

    this.dispatchEvent(
      new CustomEvent(ANTH_TAB_CHANGE_EVENT, {
        detail: { id: tab.id },
        bubbles: true,
        composed: true,
      }),
    )
    if (focus) this.focusTab(tab.id)
  }

  private focusTab(id: string): void {
    const buttons =
      this.renderRoot.querySelectorAll<HTMLButtonElement>('[role="tab"]')
    for (const button of buttons) {
      if (button.dataset.tab === id) {
        button.focus()
        return
      }
    }
  }

  /**
   * Arrow keys move between tabs, as the ARIA tabs pattern expects; Home and
   * End jump to the edges. Disabled tabs are skipped.
   */
  private onKeydown(event: KeyboardEvent): void {
    const usable = this.tabs.filter((tab) => !tab.disabled)
    if (usable.length === 0) return
    const index = usable.findIndex((tab) => tab.id === this.active)

    let next: number
    switch (event.key) {
      case 'ArrowRight':
        next = (index + 1) % usable.length
        break
      case 'ArrowLeft':
        next = (index - 1 + usable.length) % usable.length
        break
      case 'Home':
        next = 0
        break
      case 'End':
        next = usable.length - 1
        break
      default:
        return
    }

    event.preventDefault()
    this.select(usable[next].id, true)
  }

  protected override updated(changed: PropertyValues): void {
    if (changed.has('active')) this.applyVisibility()
  }

  override render() {
    return html`
      <div class="strip" role="tablist" @keydown=${this.onKeydown}>
        ${this.tabs.map(
          (tab) => html`
            <button
              type="button"
              role="tab"
              data-tab=${tab.id}
              data-active=${tab.id === this.active}
              aria-selected=${tab.id === this.active}
              aria-disabled=${tab.disabled ? 'true' : nothing}
              title=${tab.disabled ?? nothing}
              ?disabled=${Boolean(tab.disabled)}
              tabindex=${tab.id === this.active ? 0 : -1}
              @click=${() => this.select(tab.id)}
            >
              ${tab.label}
            </button>
          `,
        )}
      </div>
      <div class="panels"><slot></slot></div>
    `
  }

  static styles = css`
    :host {
      display: flex;
      flex-direction: column;
      gap: 10px;
      min-height: 0;
    }

    .strip {
      display: flex;
      flex-wrap: wrap;
      gap: 4px;
      padding-bottom: 4px;
      border-bottom: 1px solid var(--panel-border, #223041);
    }

    button {
      padding: 5px 12px;
      border: 1px solid transparent;
      border-radius: 6px;
      background: transparent;
      color: var(--muted, #8b9bad);
      font: 12px/1.4 var(--mono, monospace);
      cursor: pointer;
    }

    button:hover:not(:disabled) {
      color: var(--text, #dbe6f2);
    }

    button[data-active='true'] {
      border-color: var(--panel-border, #223041);
      background: #172231;
      color: var(--accent, #7dd3fc);
    }

    button:disabled {
      opacity: 0.4;
      cursor: default;
    }

    button:focus-visible {
      outline: 2px solid var(--accent, #7dd3fc);
      outline-offset: 2px;
    }

    .panels {
      flex: 1;
      min-height: 0;
    }
  `
}

function readStored(key: string): string | undefined {
  try {
    return globalThis.localStorage?.getItem(STORAGE_PREFIX + key) ?? undefined
  } catch {
    // A blocked storage API only costs the remembered tab.
    return undefined
  }
}

function writeStored(key: string, value: string): void {
  try {
    globalThis.localStorage?.setItem(STORAGE_PREFIX + key, value)
  } catch {
    // Same: the selection still applies to this page.
  }
}

declare global {
  interface HTMLElementTagNameMap {
    'anth-tab-panel': AnthTabPanel
  }
}
