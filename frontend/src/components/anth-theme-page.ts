import { LitElement, css, html } from 'lit'
import type { PropertyValues } from 'lit'
import { customElement, property } from 'lit/decorators.js'

import './anth-tab-panel'
import type { AnthTabPanel } from './anth-tab-panel'
import { ANTH_TAB_CHANGE_EVENT } from './anth-tab-panel'
import type { PanelDefinition, ThemeDefinition } from '../app/panels'

/** Fired when a tab is selected, with the panel id in `detail.panel`. */
export const ANTH_PANEL_SELECT_EVENT = 'anth-panel-select'

/**
 * One sidebar theme and the tab strip of its panels.
 *
 * The shell owns routing and the panel cache; this element owns one theme's
 * tab strip. It builds the tab panel once, appends one feature element per
 * panel, and from then on only changes `active`. That matters because
 * `anth-tab-panel` parks a hidden tab by removing it from the light DOM: the
 * element and its state survive, while its SSE stream or WebSocket closes.
 *
 * A tab change does not write the URL here. It emits `anth-panel-select`, and
 * the shell turns that into a hash so the address bar stays the source of
 * truth and back/forward keep working.
 */
@customElement('anth-theme-page')
export class AnthThemePage extends LitElement {
  @property({ attribute: false })
  theme?: ThemeDefinition

  @property({ attribute: false })
  panels: PanelDefinition[] = []

  /** Panel id of the tab the route selected. */
  @property({ attribute: false })
  active = ''

  /** Builds the feature element for one panel. Called once per panel. */
  @property({ attribute: false })
  createPanel?: (panel: PanelDefinition) => HTMLElement

  private tabPanel?: AnthTabPanel

  /** Theme + panel ids the current tab panel was built for. */
  private builtKey = ''

  override render() {
    return html`
      <section class="theme">
        <header class="head">
          <h1 class="title">${this.theme?.title() ?? ''}</h1>
          <p class="description">${this.theme?.description() ?? ''}</p>
        </header>
        <div class="workspace"></div>
      </section>
    `
  }

  protected override updated(_changed: PropertyValues<this>): void {
    this.sync()
  }

  private sync(): void {
    const theme = this.theme
    const workspace = this.renderRoot.querySelector<HTMLElement>('.workspace')
    if (!theme || !workspace) {
      this.removeTabs()
      return
    }

    const key = `${theme.id}|${this.panels.map((panel) => panel.id).join(',')}`
    if (!this.tabPanel || key !== this.builtKey) {
      this.build(theme, workspace, key)
    }

    // The route wins over any stale tab id. An empty or unknown id falls back
    // to the first panel so a hand-edited URL still shows something.
    const next = this.panels.some((panel) => panel.id === this.active)
      ? this.active
      : (this.panels[0]?.id ?? '')
    if (this.tabPanel && next && this.tabPanel.active !== next) {
      this.tabPanel.active = next
    }
  }

  private build(
    theme: ThemeDefinition,
    workspace: HTMLElement,
    key: string,
  ): void {
    this.removeTabs()

    const tabs = document.createElement('anth-tab-panel') as AnthTabPanel
    tabs.setAttribute('aria-label', theme.title())

    for (const panel of this.panels) {
      const section = document.createElement('section')
      section.dataset.tab = panel.id
      section.setAttribute('label', panel.title())
      section.append(
        this.createPanel?.(panel) ?? document.createElement('div'),
      )
      tabs.append(section)
    }

    // Set `active` before connecting: the tab panel selects the first tab on
    // connect, and switching afterwards would briefly mount the wrong panel.
    const next = this.panels.some((panel) => panel.id === this.active)
      ? this.active
      : (this.panels[0]?.id ?? '')
    if (next) tabs.active = next

    tabs.addEventListener(ANTH_TAB_CHANGE_EVENT, this.onTabChange)
    workspace.append(tabs)
    this.tabPanel = tabs
    this.builtKey = key
  }

  private removeTabs(): void {
    if (!this.tabPanel) return
    this.tabPanel.removeEventListener(ANTH_TAB_CHANGE_EVENT, this.onTabChange)
    this.tabPanel.remove()
    this.tabPanel = undefined
    this.builtKey = ''
  }

  private readonly onTabChange = (event: Event): void => {
    const panel = (event as CustomEvent<{ id?: string }>).detail?.id
    if (!panel) return
    this.dispatchEvent(
      new CustomEvent(ANTH_PANEL_SELECT_EVENT, {
        detail: { panel },
        bubbles: true,
        composed: true,
      }),
    )
  }

  static override styles = css`
    :host {
      display: block;
      min-width: 0;
    }

    .theme {
      display: flex;
      flex-direction: column;
      gap: 1rem;
      min-width: 0;
    }

    .head {
      display: flex;
      flex-direction: column;
      gap: 0.3rem;
    }

    .title {
      margin: 0;
      color: var(--text, #dbe6f2);
      font: 600 20px/1.25 var(--mono, ui-monospace, monospace);
      letter-spacing: 0.01em;
    }

    .description {
      margin: 0;
      max-width: 70ch;
      color: var(--muted, #8b9bad);
      font-size: 13px;
    }

    .workspace {
      min-width: 0;
    }

    @media (orientation: portrait) {
      .theme {
        gap: 0.75rem;
      }

      .title {
        font-size: 18px;
      }

      .description {
        font-size: 12px;
      }
    }
  `
}

declare global {
  interface HTMLElementTagNameMap {
    'anth-theme-page': AnthThemePage
  }
}
