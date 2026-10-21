/**
 * The panel shell: a header, a home page of feature cards and a host for the
 * selected panel.
 *
 * Navigation is hash based and each card is a real anchor, so a panel link can
 * be opened in a new tab, bookmarked, or later moved into a floating window
 * without the shell changing.
 *
 * The shell does not import any feature component directly: it asks
 * `panels.ts` for an entry and appends that custom element. That keeps the shell
 * stable while features are added, renamed or wrapped.
 */

import { LitElement, css, html } from 'lit'
import type { PropertyValues } from 'lit'
import { customElement, property, state } from 'lit/decorators.js'
import { localized, msg } from '@lit/localize'

import '../components/anth-login'

import { AuthClient } from '../lib/auth/auth-client'
import type { SessionInfo } from '../lib/auth/auth-client'
import { changeLocale, currentLocale, setupLocalization } from './i18n'
import type { PanelDefinition } from './panels'
import { useApiFetch, withRetry } from '../lib/http'
import { PANELS, findPanel } from './panels'
import type { PanelLocale } from './i18n'
import type { Route } from './router'
import { panelHash, parseRoute } from './router'

setupLocalization()

@customElement('anth-app')
@localized()
export class AnthApp extends LitElement {
  static override styles = css`
    :host {
      display: block;
      min-height: 100svh;
      color: var(--text, #dbe6f2);
      font: 15px/1.6 var(--sans, system-ui, sans-serif);
      background: var(--page-bg, #070a0f);
    }

    header {
      display: flex;
      flex-wrap: wrap;
      gap: 1rem;
      align-items: center;
      justify-content: space-between;
      padding: 1.1rem clamp(1rem, 4vw, 2.5rem);
      border-bottom: 1px solid var(--panel-border, #223041);
      background: rgba(16, 23, 33, 0.72);
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

    main {
      width: min(1180px, 100%);
      margin: 0 auto;
      padding: clamp(1.5rem, 4vw, 3rem) clamp(1rem, 4vw, 2.5rem) 4rem;
    }

    .lede {
      max-width: 60ch;
      margin: 0 0 2rem;
      color: var(--muted, #8b9bad);
    }

    .cards {
      display: grid;
      grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
      gap: 1rem;
      margin: 0;
      padding: 0;
      list-style: none;
    }

    .card {
      display: grid;
      grid-template-rows: auto auto 1fr;
      gap: 0.35rem;
      height: 100%;
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

    .card:hover,
    .card:focus-visible {
      border-color: rgba(125, 211, 252, 0.55);
      background: #16202c;
      transform: translateY(-1px);
      outline: none;
    }

    .card:focus-visible {
      box-shadow: 0 0 0 2px rgba(125, 211, 252, 0.35);
    }

    .card-icon {
      font-size: 1.4rem;
      line-height: 1;
    }

    .card-title {
      font: 600 14px/1.3 var(--mono, monospace);
      color: var(--accent, #7dd3fc);
    }

    .card-description {
      margin: 0;
      color: var(--muted, #8b9bad);
      font-size: 13px;
    }

    .panel-toolbar {
      display: flex;
      flex-wrap: wrap;
      gap: 0.8rem;
      align-items: center;
      margin-bottom: 1.2rem;
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

    .panel-title {
      margin: 0;
      font: 600 18px/1.3 var(--mono, monospace);
    }

    .unknown {
      padding: 1rem 1.1rem;
      border: 1px solid var(--err, #f87171);
      border-radius: 10px;
      color: var(--err, #f87171);
      background: rgba(248, 113, 113, 0.08);
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
  `

  /**
   * Builds the client for a panel. Tests and the future window manager replace
   * it; the default is the panel's own factory.
   */
  @property({ attribute: false })
  clientFactory?: (panel: PanelDefinition) => unknown

  /**
   * Injectable auth client. The shell reads the session on connect, so tests
   * supply a stub instead of reaching a server that is not there.
   */
  @property({ attribute: false })
  authClient?: AuthClient

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

  /** Swaps in the injected client before the first session read. */
  private adoptAuthClient(): void {
    if (this.authClient) this.auth = this.authClient
  }

  /** Panel id currently mounted in the host, so a route change remounts once. */
  private mountedPanel?: string

  private readonly onHashChange = (): void => {
    this.route = parseRoute(globalThis.location?.hash ?? '')
  }

  override connectedCallback(): void {
    super.connectedCallback()
    globalThis.addEventListener?.('hashchange', this.onHashChange)
    this.adoptAuthClient()
    // Every panel client shares one retrying fetch: a tab that outlives its
    // access token recovers on the next call instead of waiting for the timer.
    useApiFetch(withRetry(globalThis.fetch.bind(globalThis), this.auth))
    this.auth.onExpired(this.onSessionExpired)
    void this.loadSession()
  }

  override disconnectedCallback(): void {
    globalThis.removeEventListener?.('hashchange', this.onHashChange)
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
    } catch {
      this.session = null
    }
  }

  private readonly onSessionExpired = (): void => {
    this.session = null
  }

  private async onAuthenticated(session: SessionInfo): Promise<void> {
    this.session = session
    this.mountedPanel = undefined
  }

  private async onLogout(): Promise<void> {
    await this.auth.logout()
    this.session = null
    this.mountedPanel = undefined
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
      <header>
        <p class="brand">
          <a href="#/">Anthracite Panel</a>
          <span class="tagline">${msg('host control')}</span>
        </p>
        <div class="header-actions">
          ${this.session.enabled
            ? html`<span class="tagline">${this.session.username}</span>
                <button class="signout" type="button" @click=${this.onLogout}>
                  ${msg('Sign out')}
                </button>`
            : null}
          <label>
            <span class="tagline">${msg('Language')}</span>
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
      <main>${this.renderRoute()}</main>
    `
  }

  private renderRoute() {
    switch (this.route.kind) {
      case 'home':
        return this.renderHome()
      case 'panel':
        return this.renderPanel(this.route.panel)
      case 'unknown':
        return html`<p class="unknown" role="alert">
          ${msg('No such page.')} <a class="back" href="#/">${msg('Go home')}</a>
        </p>`
    }
  }

  private renderHome() {
    return html`
      <h1 class="panel-title">${msg('Control panel')}</h1>
      <p class="lede">
        ${msg(
          'Pick a feature. Every panel talks to the local daemon over the panel API.',
        )}
      </p>
      <ul class="cards">
        ${PANELS.map(
          (panel) => html`
            <li>
              <a class="card" href=${panelHash(panel.id)}>
                <span class="card-icon" aria-hidden="true">${panel.icon}</span>
                <span class="card-title">${panel.title()}</span>
                <span class="card-description">${panel.description()}</span>
              </a>
            </li>
          `,
        )}
      </ul>
    `
  }

  private renderPanel(id: string) {
    const panel = findPanel(id)
    if (!panel) {
      return html`<p class="unknown" role="alert">
        ${msg('No such panel.')} <a class="back" href="#/">${msg('Go home')}</a>
      </p>`
    }

    return html`
      <div class="panel-toolbar">
        <a class="back" href="#/">← ${msg('All panels')}</a>
        <h1 class="panel-title">${panel.title()}</h1>
      </div>
      ${this.renderFeature(panel)}
    `
  }

  private renderFeature(panel: PanelDefinition) {
    return html`<div
      class="panel-host"
      data-panel=${panel.id}
    ></div>`
  }

  /**
   * Mounts the feature element after the route's markup exists.
   *
   * A custom element tag name is data, not markup, so `lit-html` cannot bind
   * it. The host `div` is declared in the template and the element is appended
   * here, which also means the element is rebuilt only when the panel changes:
   * a locale switch re-renders the labels without dropping the live session.
   */
  protected override updated(changed: PropertyValues): void {
    if (!changed.has('route')) return

    const id = this.route.kind === 'panel' ? this.route.panel : undefined
    if (id === this.mountedPanel) return
    this.mountedPanel = id

    const host = this.renderRoot.querySelector<HTMLElement>('.panel-host')
    if (!host) return
    host.replaceChildren()

    const panel = id ? findPanel(id) : undefined
    if (panel) host.append(this.createFeatureElement(panel))
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
  }
}

declare global {
  interface HTMLElementTagNameMap {
    'anth-app': AnthApp
  }
}
