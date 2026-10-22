/**
 * The panel shell: the current header, a persistent themed sidebar and a
 * workspace whose active theme is a tab strip of its panels.
 *
 * Navigation is hash based. `#/panel/<id>` names the panel and therefore the
 * theme; `#/` is Home. The sidebar only lists themes, and the tabs inside a
 * theme list its panels. Each theme page is cached and detached, so switching
 * themes does not throw away a tab's state.
 *
 * The floating desktop and its pop-out route are intentionally not used here.
 * `anth-desktop`, `anth-window`, `windowUrl` and the router's `window` kind are
 * still kept as utilities for a later window mode; the shell simply does not
 * reach them, and `#/window/<id>` is an unknown page.
 */

import { LitElement, css, html, nothing } from 'lit'
import type { PropertyValues } from 'lit'
import { customElement, property, state } from 'lit/decorators.js'
import { localized, msg } from '@lit/localize'

import '../components/anth-login'
import '../components/anth-nav-drawer'
import '../components/anth-plugin-slot'
import '../components/anth-theme-page'
import type { AnthThemePage } from '../components/anth-theme-page'
import { ANTH_PANEL_SELECT_EVENT } from '../components/anth-theme-page'

import { AuthClient } from '../lib/auth/auth-client'
import type { SessionInfo } from '../lib/auth/auth-client'
import { changeLocale, currentLocale, setupLocalization } from './i18n'
import type { PanelLocale } from './i18n'
import type { PanelDefinition, ThemeDefinition } from './panels'
import { THEMES, findPanel, findTheme, panelsIn, themeForPanel } from './panels'
import { currentApiFetch, useApiFetch, withRetry } from '../lib/http'
import type { PluginClient, PluginsCatalogue } from '../lib/plugins/plugin-client'
import { HttpPluginClient } from '../lib/plugins/plugin-client'
import type { Route } from './router'
import { panelHash, parseRoute } from './router'
import { rememberedPanel, rememberPanel } from '../lib/ui/theme-memory'

setupLocalization()

@customElement('anth-app')
@localized()
export class AnthApp extends LitElement {
  static override styles = css`
    :host {
      display: flex;
      flex-direction: column;
      height: 100svh;
      height: 100dvh;
      overflow: hidden;
      color: var(--text, #dbe6f2);
      font: 15px/1.6 var(--sans, system-ui, sans-serif);
      background: var(--page-bg, #070a0f);
    }

    .skip {
      position: absolute;
      top: -10rem;
      left: 0.5rem;
      z-index: 20;
      padding: 0.5rem 0.75rem;
      color: var(--text, #dbe6f2);
      border: 1px solid var(--accent, #7dd3fc);
      border-radius: 7px;
      background: var(--panel, #101721);
      font: 600 12px/1.3 var(--mono, monospace);
    }

    .skip:focus {
      top: 0.5rem;
    }

    header {
      display: flex;
      flex: 0 0 auto;
      flex-wrap: wrap;
      gap: 1rem;
      align-items: center;
      justify-content: space-between;
      padding: 1.1rem clamp(1rem, 4vw, 2.5rem);
      padding-top: max(1.1rem, env(safe-area-inset-top));
      border-bottom: 1px solid var(--panel-border, #223041);
      background: rgba(16, 23, 33, 0.72);
    }

    .header-main {
      display: flex;
      align-items: center;
      gap: 0.6rem;
      min-width: 0;
    }

    .menu {
      display: none;
      place-items: center;
      width: 42px;
      height: 42px;
      padding: 0;
      color: inherit;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 9px;
      background: transparent;
      font: 400 20px/1 var(--sans, sans-serif);
      cursor: pointer;
    }

    .menu:hover,
    .menu:focus-visible {
      border-color: rgba(125, 211, 252, 0.55);
      outline: none;
    }

    .menu:focus-visible {
      box-shadow: 0 0 0 2px rgba(125, 211, 252, 0.35);
    }

    .brand {
      display: flex;
      align-items: baseline;
      gap: 0.6rem;
      margin: 0;
      font: 600 17px/1.2 var(--mono, ui-monospace, monospace);
      letter-spacing: 0.02em;
    }

    .brand a {
      color: inherit;
      text-decoration: none;
    }

    .tagline {
      color: var(--muted, #8b9bad);
      font: 12px/1.4 var(--mono, monospace);
      letter-spacing: 0.08em;
      text-transform: uppercase;
    }

    .header-actions {
      display: flex;
      align-items: center;
      gap: 0.6rem;
    }

    .shell {
      display: grid;
      grid-template-columns: clamp(210px, 18vw, 250px) minmax(0, 1fr);
      flex: 1 1 auto;
      min-height: 0;
      overflow: hidden;
    }

    .sidebar {
      align-self: stretch;
      height: 100%;
      min-height: 0;
      overflow-y: auto;
      padding: 1.2rem 0.75rem;
      padding-left: max(0.75rem, env(safe-area-inset-left));
      border-right: 1px solid var(--panel-border, #223041);
      background: rgba(12, 17, 24, 0.72);
    }

    .sidebar nav {
      display: grid;
      gap: 0.25rem;
    }

    .eyebrow {
      margin: 1rem 0.65rem 0.35rem;
      color: var(--muted, #8b9bad);
      font: 10px/1.3 var(--mono, monospace);
      letter-spacing: 0.12em;
      text-transform: uppercase;
    }

    .nav-item {
      display: flex;
      align-items: center;
      gap: 0.6rem;
      padding: 0.55rem 0.65rem;
      color: var(--muted, #8b9bad);
      text-decoration: none;
      border-left: 2px solid transparent;
      border-radius: 0 8px 8px 0;
      font: 600 12px/1.3 var(--mono, monospace);
      letter-spacing: 0.02em;
      transition:
        color 100ms ease,
        background-color 100ms ease,
        border-color 100ms ease;
    }

    .nav-item:hover,
    .nav-item:focus-visible {
      color: var(--text, #dbe6f2);
      background: rgba(125, 211, 252, 0.06);
      outline: none;
    }

    .nav-item:focus-visible {
      box-shadow: inset 0 0 0 1px rgba(125, 211, 252, 0.45);
    }

    .nav-item[data-active] {
      color: var(--accent, #7dd3fc);
      border-left-color: var(--accent, #7dd3fc);
      background: rgba(125, 211, 252, 0.1);
    }

    .nav-icon {
      width: 1.4rem;
      text-align: center;
      font-size: 1.05rem;
      line-height: 1;
    }

    .nav-label {
      flex: 1;
    }

    .nav-count {
      padding: 0.05rem 0.35rem;
      color: var(--muted, #8b9bad);
      border: 1px solid var(--panel-border, #223041);
      border-radius: 999px;
      font-size: 10px;
      font-weight: 400;
    }

    main {
      min-width: 0;
      min-height: 0;
      overflow-y: auto;
      overscroll-behavior: contain;
      -webkit-overflow-scrolling: touch;
      padding: clamp(1.25rem, 3vw, 2.4rem);
      padding-right: max(clamp(1.25rem, 3vw, 2.4rem), env(safe-area-inset-right));
      padding-bottom: max(1.5rem, env(safe-area-inset-bottom));
      padding-left: max(clamp(1.25rem, 3vw, 2.4rem), env(safe-area-inset-left));
    }

    main:focus {
      outline: none;
    }

    .page-head {
      margin-bottom: 1.5rem;
    }

    .page-title {
      margin: 0;
      font: 600 22px/1.25 var(--mono, ui-monospace, monospace);
      letter-spacing: 0.01em;
    }

    .page-lede {
      max-width: 70ch;
      margin: 0.35rem 0 0;
      color: var(--muted, #8b9bad);
      font-size: 13px;
    }

    .cards {
      display: grid;
      grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
      gap: 1rem;
      margin: 0;
      padding: 0;
      list-style: none;
    }

    .cards > li {
      display: flex;
      min-width: 0;
    }

    .card {
      display: flex;
      flex: 1 1 auto;
      flex-direction: column;
      gap: 0.35rem;
      width: 100%;
      min-width: 0;
      padding: 1.1rem 1.15rem;
      color: inherit;
      text-decoration: none;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 12px;
      background: rgba(16, 23, 33, 0.72);
      transition:
        border-color 120ms ease,
        transform 120ms ease,
        background-color 120ms ease;
    }

    @media (hover: hover) {
      .card:hover {
        border-color: rgba(125, 211, 252, 0.55);
        background: #16202c;
        transform: translateY(-1px);
      }
    }

    .card:focus-visible {
      border-color: rgba(125, 211, 252, 0.55);
      background: #16202c;
      outline: none;
      box-shadow: 0 0 0 2px rgba(125, 211, 252, 0.35);
    }

    .card-icon {
      font-size: 1.4rem;
      line-height: 1;
    }

    .card-title {
      color: var(--accent, #7dd3fc);
      font: 600 14px/1.3 var(--mono, monospace);
      overflow-wrap: anywhere;
    }

    .card-description {
      margin: 0;
      color: var(--muted, #8b9bad);
      font-size: 13px;
      overflow-wrap: anywhere;
    }

    .card-meta {
      margin-top: auto;
      color: var(--muted, #8b9bad);
      font: 10px/1.4 var(--mono, monospace);
      letter-spacing: 0.02em;
      overflow-wrap: anywhere;
    }

    .plugin-widgets {
      display: grid;
      grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
      gap: 1rem;
      margin-top: 1.5rem;
    }

    .theme-host {
      min-width: 0;
    }

    .unknown {
      padding: 1rem 1.1rem;
      color: var(--err, #f87171);
      border: 1px solid var(--err, #f87171);
      border-radius: 10px;
      background: rgba(248, 113, 113, 0.08);
    }

    .back {
      color: var(--accent, #7dd3fc);
      font: 600 12px/1.4 var(--mono, monospace);
      letter-spacing: 0.06em;
      text-transform: uppercase;
      text-decoration: none;
    }

    .back:hover {
      text-decoration: underline;
    }

    .loading {
      padding: 2rem;
      color: var(--muted, #8b9bad);
      font: 13px/1.5 var(--mono, monospace);
    }

    .signout {
      padding: 0.25rem 0.6rem;
      color: inherit;
      font: 600 11px/1.4 var(--mono, monospace);
      letter-spacing: 0.06em;
      text-transform: uppercase;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 7px;
      background: transparent;
      cursor: pointer;
    }

    .signout:hover {
      border-color: rgba(125, 211, 252, 0.55);
    }

    select {
      padding: 0.3rem 0.5rem;
      color: inherit;
      font: 12px/1.2 var(--mono, monospace);
      border: 1px solid var(--panel-border, #223041);
      border-radius: 7px;
      background: var(--panel, #101721);
    }

    .drawer-nav {
      display: grid;
      gap: 0.3rem;
    }

    .drawer-nav .nav-item {
      min-height: 48px;
      padding: 0.75rem;
      border-left: 2px solid transparent;
      border-radius: 8px;
      font-size: 13px;
    }

    .drawer-nav .nav-count {
      display: none;
    }

    @media (orientation: portrait) {
      .shell {
        display: flex;
        flex-direction: column;
      }

      .sidebar {
        display: none;
      }

      .menu {
        display: grid;
      }

      header {
        gap: 0.5rem;
        padding:
          0.7rem
          max(0.8rem, env(safe-area-inset-right))
          0.7rem
          max(0.8rem, env(safe-area-inset-left));
        padding-top: max(0.7rem, env(safe-area-inset-top));
      }

      .brand {
        font-size: 15px;
      }

      .brand-tagline,
      .language-label {
        display: none;
      }

      .header-actions {
        gap: 0.4rem;
      }

      main {
        order: 1;
        flex: 1 1 auto;
        padding: 0.8rem;
        padding-right: max(0.8rem, env(safe-area-inset-right));
        padding-bottom: max(0.8rem, env(safe-area-inset-bottom));
        padding-left: max(0.8rem, env(safe-area-inset-left));
      }

      .page-title {
        font-size: 20px;
      }

      .page-head {
        margin-bottom: 1rem;
      }

      .cards {
        grid-template-columns: 1fr;
        gap: 0.75rem;
      }

      .card {
        padding: 0.9rem;
      }

      .plugin-widgets {
        grid-template-columns: 1fr;
        margin-top: 1rem;
      }
    }
  `

  /**
   * Builds the client for a panel. Tests replace it; the default is the
   * panel's own factory.
   */
  @property({ attribute: false })
  clientFactory?: (panel: PanelDefinition) => unknown

  /**
   * Injectable auth client. The shell reads the session on connect, so tests
   * supply a stub instead of reaching a server that is not there.
   */
  @property({ attribute: false })
  authClient?: AuthClient

  /** Injectable plugin client, so a test never reaches a real `/api/plugins`. */
  @property({ attribute: false })
  pluginClient?: PluginClient

  @state()
  private route: Route = parseRoute(globalThis.location?.hash ?? '')

  @state()
  private locale: PanelLocale = currentLocale()

  /**
   * The login gate. `unknown` until the first session read finishes, so the
   * shell shows neither the login nor the panels and cannot flash protected
   * content before redirecting.
   */
  @state()
  private session: SessionInfo | null | 'unknown' = 'unknown'

  private auth = new AuthClient()

  private plugins: PluginClient = new HttpPluginClient()

  /**
   * The plugin catalogue, read once per session.
   *
   * `undefined` means "not read yet", which is also "mount no anchors": an
   * anchor whose module was never loaded is a `createElement` for an unknown
   * tag, which renders nothing and logs nothing useful.
   */
  @state()
  private catalogue?: PluginsCatalogue

  /** Portrait-only navigation drawer. */
  @state()
  private drawerOpen = false

  /**
   * One theme page per sidebar group.
   *
   * A theme page is detached, not destroyed, when another theme opens. Its
   * tab panel keeps the panel elements, so returning to a theme restores the
   * tab and its state instead of starting a second stream against the daemon.
   */
  private readonly themePages = new Map<string, AnthThemePage>()

  /** Tracks the portrait/landscape switch so a rotation closes the drawer. */
  private portraitQuery?: MediaQueryList

  /** Swaps in the injected client before the first session read. */
  private adoptAuthClient(): void {
    if (this.authClient) this.auth = this.authClient
  }

  private readonly onHashChange = (): void => {
    this.route = parseRoute(globalThis.location?.hash ?? '')
  }

  private readonly onOrientationChange = (): void => {
    if (!this.portraitQuery?.matches) this.drawerOpen = false
  }

  private readonly openDrawer = (): void => {
    this.drawerOpen = true
  }

  private readonly onDrawerClose = (): void => {
    this.drawerOpen = false
  }

  override connectedCallback(): void {
    super.connectedCallback()
    globalThis.addEventListener?.('hashchange', this.onHashChange)
    this.portraitQuery = globalThis.matchMedia?.('(orientation: portrait)')
    this.portraitQuery?.addEventListener?.('change', this.onOrientationChange)
    this.adoptAuthClient()
    // Every panel client shares one retrying fetch: a tab that outlives its
    // access token recovers on the next call instead of waiting for the timer.
    useApiFetch(withRetry(globalThis.fetch.bind(globalThis), this.auth))
    this.auth.onExpired(this.onSessionExpired)
    void this.loadSession()
  }

  override disconnectedCallback(): void {
    globalThis.removeEventListener?.('hashchange', this.onHashChange)
    this.portraitQuery?.removeEventListener?.('change', this.onOrientationChange)
    this.auth.cancelRefresh()
    useApiFetch(undefined)
    super.disconnectedCallback()
  }

  /**
   * Reads the session once at start.
   *
   * The refresh token may outlive the access token across a page reload, so a
   * failure here is not final: the client tries one rotation before deciding the
   * operator has to sign in again.
   */
  private async loadSession(): Promise<void> {
    try {
      let session = await this.auth.session()
      if (session.enabled && !session.authenticated) {
        const refreshable = session.refresh_expires_in > 0
        if (refreshable) session = await this.auth.refresh()
      }
      this.session = session
      this.auth.scheduleRefresh(session)
      void this.loadPlugins()
    } catch {
      this.session = null
    }
  }

  private readonly onSessionExpired = (): void => {
    this.session = null
    this.themePages.clear()
    this.drawerOpen = false
  }

  private async onAuthenticated(session: SessionInfo): Promise<void> {
    this.session = session
    this.themePages.clear()
    this.drawerOpen = false
    void this.loadPlugins()
  }

  /**
   * Reads the plugin catalogue after the session is known.
   *
   * A failure leaves `catalogue` undefined rather than empty, so the shell
   * renders its built-in themes and no plugin slots, and the operator is not
   * told their dashboard is empty when the daemon merely failed a read.
   */
  private async loadPlugins(): Promise<void> {
    this.plugins =
      this.pluginClient ?? new HttpPluginClient({ fetchImpl: currentApiFetch() })
    try {
      this.catalogue = await this.plugins.list()
    } catch {
      this.catalogue = undefined
    }
  }

  private async onLogout(): Promise<void> {
    await this.auth.logout()
    this.session = null
    this.themePages.clear()
    this.catalogue = undefined
    this.drawerOpen = false
  }

  override render() {
    if (this.session === 'unknown') {
      return html`<p class="loading">${msg('Loading…')}</p>`
    }
    if (this.session === null || !this.session.authenticated) {
      return html`<anth-login
        .client=${this.auth}
        .onAuthenticated=${(session: SessionInfo) => this.onAuthenticated(session)}
      ></anth-login>`
    }

    return html`
      <a class="skip" href="#main" @click=${this.skipToMain}>
        ${msg('Skip to content')}
      </a>
      <header>
        <div class="header-main">
          <button
            class="menu"
            type="button"
            aria-label=${msg('Open navigation')}
            aria-controls="app-navigation"
            aria-expanded=${this.drawerOpen ? 'true' : 'false'}
            @click=${this.openDrawer}
          >
            ☰
          </button>
          <p class="brand">
            <a href="#/">Anthracite Panel</a>
            <span class="tagline brand-tagline">${msg('host control')}</span>
          </p>
        </div>
        <div class="header-actions">
          ${this.session.enabled
            ? html`<span class="tagline">${this.session.username}</span>
                <button class="signout" type="button" @click=${this.onLogout}>
                  ${msg('Sign out')}
                </button>`
            : null}
          <label>
            <span class="tagline language-label">${msg('Language')}</span>
            <select
              aria-label=${msg('Language')}
              .value=${this.locale}
              @change=${this.onLocaleChange}
            >
              <option value="en">${msg('English')}</option>
              <option value="es">${msg('Spanish')}</option>
            </select>
          </label>
        </div>
      </header>
      <div class="shell">
        <aside class="sidebar">
          <nav aria-label=${msg('Themes')}>
            ${this.renderHomeLink()}
            <p class="eyebrow">${msg('Themes')}</p>
            ${THEMES.map((theme) => this.renderThemeLink(theme))}
          </nav>
        </aside>
        <main id="main" tabindex="-1">${this.renderRoute()}</main>
      </div>
      <anth-nav-drawer
        id="app-navigation"
        .open=${this.drawerOpen}
        @anth-drawer-close=${this.onDrawerClose}
      >
        <nav class="drawer-nav" aria-label=${msg('Themes')}>
          ${this.renderHomeLink(true)}
          ${THEMES.map((theme) => this.renderThemeLink(theme, true))}
        </nav>
      </anth-nav-drawer>
    `
  }

  private renderHomeLink(short = false) {
    return html`
      <a
        class="nav-item"
        href="#/"
        ?data-active=${this.route.kind === 'home'}
        aria-current=${this.route.kind === 'home' ? 'page' : nothing}
        title=${short ? msg('Home') : nothing}
      >
        <span class="nav-icon" aria-hidden="true">🏠</span>
        <span class="nav-label">${msg('Home')}</span>
      </a>
    `
  }

  private renderThemeLink(theme: ThemeDefinition, short = false) {
    const landing = this.landingPanel(theme)
    if (!landing) return nothing
    const active =
      this.route.kind === 'panel' &&
      themeForPanel(this.route.panel)?.id === theme.id
    const count = panelsIn(theme.id).length

    return html`
      <a
        class="nav-item"
        href=${panelHash(landing.id)}
        data-theme=${theme.id}
        ?data-active=${active}
        aria-current=${active ? 'page' : nothing}
        aria-label=${short ? theme.title() : nothing}
        title=${short ? theme.title() : nothing}
      >
        <span class="nav-icon" aria-hidden="true">${theme.icon}</span>
        <span class="nav-label"
          >${short ? theme.shortTitle() : theme.title()}</span
        >
        <span class="nav-count" aria-hidden="true">${count}</span>
      </a>
    `
  }

  private renderRoute() {
    switch (this.route.kind) {
      case 'home':
        return this.renderHome()
      case 'panel':
        return this.renderPanel(this.route.panel)
      case 'window':
        // The pop-out route is deliberately not part of this shell anymore.
        // The desktop and its parser remain as utilities for a later window
        // mode; an old URL is treated as unknown rather than redirecting.
        return this.renderUnknownPage()
      case 'unknown':
        return this.renderUnknownPage()
    }
  }

  private renderHome() {
    return html`
      <section class="home">
        <header class="page-head">
          <h1 class="page-title">${msg('Home')}</h1>
          <p class="page-lede">
            ${msg('Choose a theme from the sidebar or open one below.')}
          </p>
        </header>
        <ul class="cards">
          ${THEMES.map((theme) => this.renderThemeCard(theme))}
        </ul>
        <section class="plugin-widgets" aria-label=${msg('Plugin widgets')}>
          <anth-plugin-slot
            slot-name="dashboard.widget"
            .catalogue=${this.catalogue}
            .client=${this.plugins}
          ></anth-plugin-slot>
        </section>
      </section>
    `
  }

  private renderThemeCard(theme: ThemeDefinition) {
    const landing = this.landingPanel(theme)
    if (!landing) return nothing
    const panels = panelsIn(theme.id)

    return html`
      <li>
        <a class="card" href=${panelHash(landing.id)} data-theme=${theme.id}>
          <span class="card-icon" aria-hidden="true">${theme.icon}</span>
          <span class="card-title">${theme.title()}</span>
          <span class="card-description">${theme.description()}</span>
          <span class="card-meta">
            ${panels.map((panel) => panel.title()).join(' · ')}
          </span>
        </a>
      </li>
    `
  }

  private renderPanel(id: string) {
    const panel = findPanel(id)
    const theme = panel ? findTheme(panel.theme) : undefined
    if (!panel || !theme) {
      return html`<p class="unknown" role="alert">
        ${msg('No such panel.')} <a class="back" href="#/">${msg('Go home')}</a>
      </p>`
    }
    return html`<div class="theme-host" data-theme=${theme.id}></div>`
  }

  private renderUnknownPage() {
    return html`<p class="unknown" role="alert">
      ${msg('No such page.')} <a class="back" href="#/">${msg('Go home')}</a>
    </p>`
  }

  /** The panel the sidebar and the Home card link to for a theme. */
  private landingPanel(theme: ThemeDefinition): PanelDefinition | undefined {
    const panels = panelsIn(theme.id)
    const stored = rememberedPanel(theme.id)
    return panels.find((panel) => panel.id === stored) ?? panels[0]
  }

  /**
   * Keeps the routed theme's page in the host, creating it on first visit and
   * reusing the cached one afterwards.
   *
   * Called from `updated` rather than `render` because the host is a DOM node
   * Lit owns, while the theme page (and its tab panel) must survive updates.
   */
  private readonly syncThemePage = (): void => {
    if (this.route.kind !== 'panel') return
    const panel = findPanel(this.route.panel)
    const theme = panel ? findTheme(panel.theme) : undefined
    if (!panel || !theme) return

    const host = this.renderRoot.querySelector<HTMLElement>('.theme-host')
    if (!host) return

    rememberPanel(theme.id, panel.id)

    let page = this.themePages.get(theme.id)
    if (!page) {
      page = document.createElement('anth-theme-page') as AnthThemePage
      page.theme = theme
      page.panels = panelsIn(theme.id)
      page.createPanel = (entry) => this.createFeatureElement(entry)
      page.addEventListener(ANTH_PANEL_SELECT_EVENT, this.onPanelSelect)
      this.themePages.set(theme.id, page)
    }

    page.active = panel.id
    if (host.firstElementChild !== page) host.replaceChildren(page)
  }

  /** A tab click becomes a hash, keeping the address bar the source of truth. */
  private readonly onPanelSelect = (event: Event): void => {
    const panel = (event as CustomEvent<{ panel?: string }>).detail?.panel
    if (!panel) return

    const entry = findPanel(panel)
    const theme = entry ? findTheme(entry.theme) : undefined
    if (theme) rememberPanel(theme.id, panel)

    if (this.route.kind === 'panel' && this.route.panel === panel) return
    globalThis.location.hash = panelHash(panel)
  }

  private readonly skipToMain = (event: Event): void => {
    event.preventDefault()
    this.renderRoot.querySelector<HTMLElement>('main')?.focus()
  }

  protected override updated(changed: PropertyValues): void {
    if (changed.has('route')) this.drawerOpen = false
    this.syncThemePage()
  }

  private createFeatureElement(panel: PanelDefinition): HTMLElement {
    const element = document.createElement(panel.element)
    const client = this.clientFactory
      ? this.clientFactory(panel)
      : panel.createClient?.()
    if (client !== undefined) {
      const property = panel.clientProperty ?? 'client'
      ;(element as unknown as Record<string, unknown>)[property] = client
    }
    for (const [key, value] of Object.entries(panel.properties ?? {})) {
      ;(element as unknown as Record<string, unknown>)[key] = value
    }
    return element
  }

  private async onLocaleChange(event: Event): Promise<void> {
    const locale = (event.target as HTMLSelectElement).value as PanelLocale
    await changeLocale(locale)
    this.locale = currentLocale()
    // Theme titles and tab labels are captured when a tab panel is built.
    // Dropping the cache rebuilds them from the new locale; the route and the
    // active panel id decide which tab is shown again.
    this.themePages.clear()
    this.requestUpdate()
  }
}

declare global {
  interface HTMLElementTagNameMap {
    'anth-app': AnthApp
  }
}
