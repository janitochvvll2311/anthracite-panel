/**
 * `anth-tab-panel`: the tab strip and the one visible tab.
 *
 * A hidden tab is **removed** from the light DOM, not set to `hidden`. A
 * detached custom element keeps its properties, its shadow root and its
 * internal state across `remove()`/`append()`, so a tab resumes where it left
 * off, while it cannot render and its `disconnectedCallback` has fired — which
 * is what closes a panel's live stream. A `display: none` tab would keep
 * rendering and keep the connection open in the background.
 *
 * Tabs are declared in the light DOM:
 *
 * ```html
 * <anth-tab-panel active="host">
 *   <section data-tab="host" label="Host">…</section>
 * </anth-tab-panel>
 * ```
 *
 * The panel never removes the strip: a theme with one panel still shows one
 * tab, because the strip is where a second tab will appear.
 */

/** Fired when the user picks a tab. The shell decides what the id means. */
export const TAB_CHANGE_EVENT = 'anth-tab-change'

export interface TabChangeDetail {
  id: string
}

interface TabDefinition {
  id: string
  label: string
}

export class AnthTabPanel extends HTMLElement {
  static get observedAttributes(): string[] {
    return ['active']
  }

  /** Id of the visible tab. An unknown or empty value selects the first. */
  get active(): string {
    return this.getAttribute('active') ?? ''
  }

  set active(value: string) {
    if (value) {
      this.setAttribute('active', value)
    } else {
      this.removeAttribute('active')
    }
  }

  /** Every declared tab, in declaration order, parked or not. */
  #elements = new Map<string, HTMLElement>()

  #mounted = false

  #observer: MutationObserver | undefined

  connectedCallback(): void {
    if (!this.shadowRoot) {
      this.attachShadow({ mode: 'open' })
    }
    if (!this.#mounted) {
      // The `<slot>` is not decoration: without it the active tab stays in the
      // light DOM, connected but never rendered, and the panel shows an empty
      // box. `isConnected` cannot catch that.
      this.shadowRoot!.innerHTML = `<style>${STYLE}</style><div class="strip" role="tablist" part="strip"></div><slot></slot>`
      this.#mounted = true
    }

    this.#collect()
    // `data-tab`, `label` and `data-disabled` are read when the strip is built,
    // so a tab that changes them has to be re-read. Only *added* children
    // matter: the removals this element causes itself are its own parking.
    this.#observer = new MutationObserver(() => {
      this.#collect()
      this.#paint()
    })
    // `subtree` stays off on purpose: the shadow root is inside this subtree, so
    // a subtree observer would see `#paint()` writing the tab strip and call
    // itself again. Only direct children declare a tab.
    this.#observer.observe(this, {
      childList: true,
      attributes: true,
      attributeFilter: ['data-tab', 'label', 'data-disabled'],
    })

    this.#paint()
  }

  disconnectedCallback(): void {
    this.#observer?.disconnect()
    this.#observer = undefined
  }

  attributeChangedCallback(name: string): void {
    if (name === 'active') {
      this.#paint()
    }
  }

  /** The element registered for `id`, whether it is on the page or parked. */
  tab(id: string): HTMLElement | undefined {
    return this.#elements.get(id)
  }

  /** Re-reads labels and repaints. The shell calls it after a locale change. */
  refresh(): void {
    this.#collect()
    this.#paint()
  }

  /** Records every declared tab. A parked tab is still in this map. */
  #collect(): void {
    for (const child of this.children) {
      if (!(child instanceof HTMLElement)) {
        continue
      }
      const id = child.dataset.tab ?? child.id
      if (!id) {
        continue
      }
      this.#elements.set(id, child)
    }
  }

  #tabs(): TabDefinition[] {
    const tabs: TabDefinition[] = []
    for (const [id, element] of this.#elements) {
      if (element.dataset.disabled !== undefined) {
        continue
      }
      tabs.push({ id, label: element.getAttribute('label') ?? element.dataset.label ?? id })
    }
    return tabs
  }

  #paint(): void {
    if (!this.shadowRoot) {
      return
    }
    const tabs = this.#tabs()
    // An id that no longer exists falls back to the first tab, so a route that
    // outlived its panel still shows something.
    const active = tabs.some((tab) => tab.id === this.active)
      ? this.active
      : (tabs[0]?.id ?? '')
    if (active !== this.active) {
      // Writing the attribute re-enters through `attributeChangedCallback`,
      // which repaints with the corrected id.
      this.active = active
    }

    const strip = this.shadowRoot.querySelector<HTMLElement>('.strip')
    if (strip) {
      strip.innerHTML = ''
      for (const tab of tabs) {
        const button = document.createElement('button')
        button.type = 'button'
        button.className = 'tab'
        button.dataset.tab = tab.id
        button.setAttribute('role', 'tab')
        button.setAttribute('aria-selected', String(tab.id === active))
        if (tab.id === active) {
          button.dataset.active = ''
        }
        button.textContent = tab.label
        button.addEventListener('click', () => this.#select(tab.id))
        strip.append(button)
      }
    }

    // Park every tab but the active one. Removal is what stops a parked panel
    // from rendering; it stays in `#elements` so it can come back.
    for (const [id, element] of this.#elements) {
      if (id === active) {
        if (!element.isConnected) {
          this.append(element)
        }
      } else if (element.isConnected) {
        element.remove()
      }
    }
  }

  #select(id: string): void {
    if (id === this.active) {
      return
    }
    this.active = id
    this.dispatchEvent(
      new CustomEvent<TabChangeDetail>(TAB_CHANGE_EVENT, {
        detail: { id },
        bubbles: true,
        composed: true,
      }),
    )
  }
}

const STYLE = `
  :host { display: block; min-width: 0; }
  .strip { display: flex; flex-wrap: wrap; gap: 0.35rem; margin-bottom: var(--gap, 1rem);
           border-bottom: 1px solid var(--panel-border, #223041); }
  .tab { min-height: 44px; padding: 0.45rem 0.8rem; color: var(--muted, #8b9bad);
         background: transparent; border: 0; border-bottom: 2px solid transparent;
         font: 600 12px/1.3 var(--mono, monospace); letter-spacing: 0.02em;
         cursor: pointer; }
  .tab:hover, .tab:focus-visible { color: var(--text, #dbe6f2); outline: none; }
  .tab:focus-visible { box-shadow: inset 0 0 0 1px var(--accent, #7dd3fc); border-radius: 4px; }
  .tab[data-active] { color: var(--accent, #7dd3fc); border-bottom-color: var(--accent, #7dd3fc); }
`

if (!customElements.get('anth-tab-panel')) {
  customElements.define('anth-tab-panel', AnthTabPanel)
}
