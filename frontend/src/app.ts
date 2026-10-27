/**
 * `anth-app`: the shell.
 *
 * The frame is a fixed-height app column: the header keeps its natural height,
 * the body takes the rest, and `main` is the only scroll container. That is what
 * keeps the title bar and the navigation in place while the content moves.
 *
 * The frame is built once and only the active panel is painted. Navigation, the
 * drawer and the tab strip all read `destinations.ts`, so a new destination is
 * declared once and appears in the sidebar, the drawer and the tab strip
 * together.
 */

import { msg } from '@lit/localize'

import type { AuthClient, AuthState } from './auth-client'
import {
  filesPanels,
  hashForPanel,
  hashForSection,
  networkPanels,
  routeFrom,
  sections,
  settingsPanels,
} from './destinations'
import type { PanelId, Route } from './destinations'
import { describeError } from './errors'
import './icon'
import { escape } from './html'
import type { AnthFiles } from './files'
import type { AnthHostInfo } from './host-info'
import type { FilesClient } from './files-client'
import type { HostInfoClient } from './host-info-client'
import type { ProcessesClient } from './processes-client'
import type { AnthProcesses } from './processes'
import type { NetworkClient } from './network-client'
import type { AnthNetwork } from './network'
import { changeLocale, currentLocale } from './i18n'
import type { PanelLocale } from './i18n'
import { DRAWER_CLOSE_EVENT } from './nav-drawer'
import type { AnthNavDrawer } from './nav-drawer'
import type { AnthSettings } from './settings'
import type { SettingsClient } from './settings-client'
import { TAB_CHANGE_EVENT } from './tab-panel'
import type { AnthTabPanel, TabChangeDetail } from './tab-panel'
import { closeTerminalWindow, openTerminalWindow } from './terminal-window'

// The shell renders these, so it imports them: a test that mounts `anth-app`
// on its own must still get working custom elements.
import './fab'
import './files'
import './host-info'
import './nav-drawer'
import './network'
import './processes'
import './settings'
import './tab-panel'

const VERSION = '0.3.0'
const SIDEBAR_KEY = 'anth.sidebar'

export class AnthApp extends HTMLElement {
  /** Set before mounting by `main`, or replaced by a test. */
  static authClient: AuthClient | undefined
  static hostClient: HostInfoClient | undefined
  static filesClient: FilesClient | undefined
  static processesClient: ProcessesClient | undefined
  static networkClient: NetworkClient | undefined
  static settingsClient: SettingsClient | undefined

  #auth?: AuthState
  #error = ''
  #busy = false
  #route: Route = routeFrom('')
  #locale: PanelLocale = 'en'
  #sidebarCollapsed = false

  #frameMounted = false
  #shellMounted = false
  #tabPanel?: AnthTabPanel
  #filesPanel?: AnthTabPanel
  #networkPanel?: AnthTabPanel
  #settingsPanel?: AnthTabPanel
  #homeSection?: HTMLElement
  #filesSection?: HTMLElement
  #terminalSection?: HTMLElement
  #processesSection?: HTMLElement
  #networkSection?: HTMLElement
  #settingsSection?: HTMLElement
  #filesTabSection?: HTMLElement
  #interfacesTabSection?: HTMLElement
  #generalTabSection?: HTMLElement
  #drawer?: AnthNavDrawer

  connectedCallback(): void {
    if (!this.shadowRoot) {
      this.attachShadow({ mode: 'open' })
    }
    this.#locale = currentLocale()
    this.#route = routeFrom(location.hash)
    try {
      this.#sidebarCollapsed =
        localStorage.getItem(SIDEBAR_KEY) === 'collapsed'
    } catch {
      // Private mode without storage: the rail still works for this session.
    }
    this.classList.toggle('sidebar-collapsed', this.#sidebarCollapsed)
    this.#buildFrame()
    globalThis.addEventListener('hashchange', this.#onHashChange)
    globalThis.addEventListener('lit-localize-status', this.#onLocaleChange)
    // The element is connected by `customElements.define` while `app.ts` is
    // evaluated, which is before the composition root assigns its clients.
    // Deferring one microtask lets that assignment happen first.
    queueMicrotask(() => void this.#load())
  }

  disconnectedCallback(): void {
    globalThis.removeEventListener('hashchange', this.#onHashChange)
    globalThis.removeEventListener('lit-localize-status', this.#onLocaleChange)
  }

  /** Re-read the auth state; the shell follows from it. */
  async load(): Promise<void> {
    await this.#load()
  }

  async #load(): Promise<void> {
    const client = AnthApp.authClient
    if (!client) {
      this.#error = msg('No auth client was configured')
      this.render()
      return
    }
    this.#busy = true
    this.render()
    try {
      this.#auth = await client.state()
      this.#error = ''
    } catch (error) {
      this.#error = describeError(error)
    }
    this.#busy = false
    this.render()
  }

  async #perform(action: () => Promise<void>): Promise<void> {
    this.#busy = true
    this.render()
    try {
      await action()
      this.#error = ''
      this.#busy = false
      await this.#load()
    } catch (error) {
      this.#error = describeError(error)
      this.#busy = false
      this.render()
    }
  }

  private ready(): boolean {
    return !!this.#auth && (!this.#auth.required || this.#auth.authenticated)
  }

  private render(): void {
    if (!this.shadowRoot) {
      return
    }
    const ready = this.ready()
    // The login/registration form owns the whole page: a sidebar behind it
    // would suggest sections the visitor cannot open yet.
    this.classList.toggle('auth-only', !ready)
    if (!ready) {
      this.#setDrawer(false)
    }
    this.#paintNav()
    this.#paintFoot()
    this.#syncSidebarToggle()

    const errorEl = this.shadowRoot.querySelector<HTMLElement>('[data-error]')
    const authEl = this.shadowRoot.querySelector<HTMLElement>('[data-auth]')
    const shellEl = this.shadowRoot.querySelector<HTMLElement>('[data-shell]')
    if (!errorEl || !authEl || !shellEl) {
      return
    }

    errorEl.textContent = this.#error
    errorEl.hidden = this.#error === ''

    if (!this.#auth) {
      this.#unmountShell(shellEl)
      shellEl.hidden = true
      authEl.hidden = false
      authEl.innerHTML = this.#error
        ? `<p class="empty"><button class="retry" type="button" data-retry aria-label="${msg('Retry')}"><anth-icon name="refresh"></anth-icon><span class="btn-label">${msg('Retry')}</span></button></p>`
        : `<p class="empty">${msg('Loading…')}</p>`
      authEl.querySelector('[data-retry]')?.addEventListener('click', () => void this.#load())
      return
    }

    if (!this.ready()) {
      this.#unmountShell(shellEl)
      shellEl.hidden = true
      authEl.hidden = false
      this.#paintAuth(authEl)
      return
    }

    authEl.hidden = true
    authEl.innerHTML = ''
    shellEl.hidden = false
    this.#ensureShell(shellEl)
    this.#paintTabs()
  }

  /** The frame's static HTML. It is written once; later paints update it. */
  #buildFrame(): void {
    const root = this.shadowRoot
    if (!root || this.#frameMounted) {
      return
    }
    root.innerHTML = `
      <style>${STYLE}</style>
      <a class="skip" href="#main" data-skip>${msg('Skip to content')}</a>
      <header>
        <div class="header-main">
          <button class="menu" type="button" aria-label="${msg('Open navigation')}"
                  aria-controls="app-navigation" aria-expanded="false"><anth-icon name="menu"></anth-icon></button>
          <h1>Anthracite Panel</h1>
          <span class="version">v${VERSION}</span>
        </div>
      </header>
      <div class="shell">
        <aside class="sidebar" id="sidebar">
          <button
            class="sidebar-toggle"
            type="button"
            data-sidebar-toggle
            aria-controls="sidebar"
            aria-expanded="true"
            aria-label="${msg('Collapse sidebar')}"
            title="${msg('Collapse sidebar')}"
          ><anth-icon name="menu"></anth-icon></button>
          <nav aria-label="${msg('Sections')}" data-sidebar></nav>
          <div class="nav-foot" data-nav-foot></div>
        </aside>
        <main id="main" tabindex="-1">
          <p class="error" data-error role="alert" hidden></p>
          <div class="auth" data-auth></div>
          <div data-shell hidden></div>
        </main>
      </div>
      <anth-nav-drawer id="app-navigation">
        <nav aria-label="${msg('Sections')}" data-drawer></nav>
        <div class="nav-foot" slot="footer" data-nav-foot></div>
      </anth-nav-drawer>
    `
    this.#frameMounted = true
    this.#drawer = root.querySelector('anth-nav-drawer') as AnthNavDrawer | null ?? undefined
    this.#wireFrame()
    this.#syncSidebarToggle()
  }

  #wireFrame(): void {
    const root = this.shadowRoot
    if (!root) {
      return
    }
    // Fragment navigation cannot cross a shadow boundary, so `href="#main"`
    // alone would only rewrite the route. Focus is moved by hand.
    root.querySelector('[data-skip]')?.addEventListener('click', (event) => {
      event.preventDefault()
      root.querySelector<HTMLElement>('main')?.focus()
    })
    root.querySelector('.menu')?.addEventListener('click', () => this.#setDrawer(true))
    root
      .querySelector('[data-sidebar-toggle]')
      ?.addEventListener('click', () => this.#toggleSidebar())
    this.#drawer?.addEventListener(DRAWER_CLOSE_EVENT, () => this.#setDrawer(false))
    // The sidebar and the drawer are repainted on every render, so the footer
    // controls are handled once, by delegation.
    root.addEventListener('click', this.#onFrameClick)
    root.addEventListener('change', this.#onFrameChange)
  }

  readonly #onFrameClick = (event: Event): void => {
    const target = event.composedPath()[0]
    if (target instanceof Element && target.closest('[data-signout]')) {
      void this.#logout()
    }
  }

  readonly #onFrameChange = (event: Event): void => {
    const target = event.composedPath()[0]
    if (target instanceof HTMLSelectElement && target.matches('[data-locale]')) {
      void this.#switchLocale(target.value as PanelLocale)
    }
  }

  #setDrawer(open: boolean): void {
    if (this.#drawer) {
      this.#drawer.open = open
    }
    this.shadowRoot?.querySelector('.menu')?.setAttribute('aria-expanded', String(open))
  }

  #toggleSidebar(): void {
    this.#setSidebarCollapsed(!this.#sidebarCollapsed)
  }

  #setSidebarCollapsed(collapsed: boolean): void {
    this.#sidebarCollapsed = collapsed
    this.classList.toggle('sidebar-collapsed', collapsed)
    try {
      localStorage.setItem(SIDEBAR_KEY, collapsed ? 'collapsed' : 'expanded')
    } catch {
      // The class already changed; persistence is best effort.
    }
    this.#syncSidebarToggle()
  }

  #syncSidebarToggle(): void {
    const button = this.shadowRoot?.querySelector<HTMLButtonElement>(
      '[data-sidebar-toggle]',
    )
    if (!button) {
      return
    }
    const label = this.#sidebarCollapsed
      ? msg('Expand sidebar')
      : msg('Collapse sidebar')
    button.setAttribute('aria-label', label)
    button.setAttribute('title', label)
    button.setAttribute('aria-expanded', String(!this.#sidebarCollapsed))
  }

  /** Language and sign-out, pinned to the bottom of the sidebar and the drawer. */
  #paintFoot(): void {
    const root = this.shadowRoot
    if (!root) {
      return
    }
    const markup = `
      <label class="locale">
        <anth-icon name="globe"></anth-icon>
        <select data-locale aria-label="${msg('Language')}">
          <option value="en">${msg('English')}</option>
          <option value="es">${msg('Spanish')}</option>
        </select>
      </label>
      <button data-signout type="button" aria-label="${msg('Sign out')}"
        ${this.ready() ? '' : 'hidden'} ${this.#busy ? 'disabled' : ''}>
        <anth-icon name="logout"></anth-icon><span class="btn-label">${msg('Sign out')}</span>
      </button>`
    for (const foot of root.querySelectorAll<HTMLElement>('[data-nav-foot]')) {
      foot.innerHTML = markup
      const select = foot.querySelector<HTMLSelectElement>('select[data-locale]')
      if (select) {
        select.value = this.#locale
      }
    }
  }

  /** One set of links, rendered twice: sidebar and drawer. */
  #paintNav(): void {
    const root = this.shadowRoot
    if (!root) {
      return
    }
    const links = sections()
      .map(
        (section) => `
          <a class="nav-item" href="${hashForSection(section.id)}" data-section="${section.id}"
             title="${escape(section.label)}" aria-label="${escape(section.label)}"
             ${section.id === this.#route.section ? 'data-active aria-current="page"' : ''}>
            <span class="nav-icon" aria-hidden="true"><anth-icon name="${escape(section.icon)}"></anth-icon></span>
            <span class="nav-label">${escape(section.label)}</span>
          </a>`,
      )
      .join('')
    const sidebar = root.querySelector('[data-sidebar]')
    const drawer = root.querySelector('[data-drawer]')
    if (sidebar) {
      sidebar.innerHTML = links
    }
    if (drawer) {
      drawer.innerHTML = links
    }
    for (const nav of root.querySelectorAll('nav')) {
      nav.setAttribute('aria-label', msg('Sections'))
    }
  }

  #paintAuth(container: HTMLElement): void {
    const auth = this.#auth
    if (!auth) {
      return
    }
    const register = auth.bootstrap === 'web' && !auth.registered
    container.innerHTML = register
      ? `
        <form class="auth-card" data-register>
          <h2>${msg('Set the first password')}</h2>
          <p class="muted">${msg('The operator account is')} <strong>${escape(auth.username)}</strong>.</p>
          <label>${msg('Password')}<input name="password" type="password" minlength="12" required autocomplete="new-password"></label>
          <label>${msg('Confirm password')}<input name="confirm" type="password" minlength="12" required autocomplete="new-password"></label>
          <button type="submit" ${this.#busy ? 'disabled' : ''} aria-label="${msg('Create password')}"><anth-icon name="key"></anth-icon><span class="btn-label">${msg('Create password')}</span></button>
        </form>`
      : `
        <form class="auth-card" data-login>
          <h2>${msg('Sign in')}</h2>
          <label>${msg('Username')}<input name="username" value="${escape(auth.username)}" autocomplete="username" required></label>
          <label>${msg('Password')}<input name="password" type="password" autocomplete="current-password" required></label>
          <button type="submit" ${this.#busy ? 'disabled' : ''} aria-label="${msg('Sign in')}"><anth-icon name="login"></anth-icon><span class="btn-label">${msg('Sign in')}</span></button>
        </form>`
    container
      .querySelector('form')
      ?.addEventListener('submit', register ? this.#onRegister : this.#onLogin)
  }

  /**
   * Mounts the two levels once: sections in a bare tab panel (the sidebar is
   * their navigation), and one tab strip per section that owns a panel.
   */
  #ensureShell(container: HTMLElement): void {
    if (this.#shellMounted) {
      return
    }
    const outer = document.createElement('anth-tab-panel') as AnthTabPanel
    // The sidebar already names the sections; a strip here would duplicate it.
    outer.setAttribute('bare', '')
    outer.setAttribute('active', this.#route.section)
    outer.innerHTML = `
      <section data-tab="home" label="${escape(msg('Home'))}">
        <section class="card" data-host-card>
          <div class="card-head">
            <span class="label">${msg('Host')}</span>
          </div>
          <anth-host-info></anth-host-info>
          <anth-fab icon="refresh" label="${escape(msg('Refresh host information'))}"></anth-fab>
        </section>
      </section>
      <section data-tab="files" label="${escape(msg('Files'))}"></section>
      <section data-tab="terminal" label="${escape(msg('Terminal'))}">
        <section class="card terminal-launcher">
          <h2>${msg('Terminal')}</h2>
          <p class="muted">${msg('Open a shell on this host. Sessions stay open while you use the panel.')}</p>
          <button type="button" class="launch-terminal" data-open-terminal>
            <anth-icon name="terminal"></anth-icon>
            <span class="btn-label">${msg('Open terminal')}</span>
          </button>
        </section>
      </section>
      <section data-tab="processes" label="${escape(msg('Processes'))}">
        <anth-processes></anth-processes>
      </section>
      <section data-tab="network" label="${escape(msg('Network'))}"></section>
      <section data-tab="settings" label="${escape(msg('Settings'))}"></section>
    `
    const homeSection = outer.querySelector<HTMLElement>('[data-tab="home"]') ?? undefined
    const filesSection = outer.querySelector<HTMLElement>('[data-tab="files"]') ?? undefined
    const terminalSection =
      outer.querySelector<HTMLElement>('[data-tab="terminal"]') ?? undefined
    const processesSection =
      outer.querySelector<HTMLElement>('[data-tab="processes"]') ?? undefined
    const networkSection =
      outer.querySelector<HTMLElement>('[data-tab="network"]') ?? undefined
    const settingsSection = outer.querySelector<HTMLElement>('[data-tab="settings"]') ?? undefined

    const filesPanel = document.createElement('anth-tab-panel') as AnthTabPanel
    filesPanel.setAttribute('active', 'files')
    filesPanel.innerHTML = filesPanels()
      .map(
        (panel) => `
          <section data-tab="${panel.id}" label="${escape(panel.label)}">
            <anth-files></anth-files>
          </section>`,
      )
      .join('')
    const filesTabSection = filesPanel.querySelector<HTMLElement>(':scope > section') ?? undefined
    filesSection?.append(filesPanel)

    const networkPanel = document.createElement('anth-tab-panel') as AnthTabPanel
    networkPanel.setAttribute('active', 'interfaces')
    networkPanel.innerHTML = networkPanels()
      .map(
        (panel) => `
          <section data-tab="${panel.id}" label="${escape(panel.label)}">
            <anth-network></anth-network>
          </section>`,
      )
      .join('')
    const interfacesTabSection =
      networkPanel.querySelector<HTMLElement>(':scope > section') ?? undefined
    networkSection?.append(networkPanel)

    const settingsPanel = document.createElement('anth-tab-panel') as AnthTabPanel
    settingsPanel.setAttribute('active', 'general')
    settingsPanel.innerHTML = settingsPanels()
      .map(
        (panel) => `
          <section data-tab="${panel.id}" label="${escape(panel.label)}">
            <anth-settings></anth-settings>
          </section>`,
      )
      .join('')
    const generalTabSection =
      settingsPanel.querySelector<HTMLElement>(':scope > section') ?? undefined
    settingsSection?.append(settingsPanel)

    // Assign the clients before the panels join the page: they start reading
    // from `connectedCallback`, so a later assignment would be a card that
    // never loads.
    const host = homeSection?.querySelector<AnthHostInfo>('anth-host-info')
    if (host) {
      host.client = AnthApp.hostClient
    }
    const files = filesPanel.querySelector<AnthFiles>('anth-files')
    if (files) {
      files.client = AnthApp.filesClient
    }
    const processes = processesSection?.querySelector<AnthProcesses>('anth-processes')
    if (processes) {
      processes.client = AnthApp.processesClient
    }
    const network = networkPanel.querySelector<AnthNetwork>('anth-network')
    if (network) {
      network.client = AnthApp.networkClient
    }
    for (const settings of settingsPanel.querySelectorAll<AnthSettings>('anth-settings')) {
      settings.client = AnthApp.settingsClient
    }

    container.append(outer)

    this.#tabPanel = outer
    this.#filesPanel = filesPanel
    this.#networkPanel = networkPanel
    this.#settingsPanel = settingsPanel
    this.#homeSection = homeSection
    this.#filesSection = filesSection
    this.#terminalSection = terminalSection
    this.#processesSection = processesSection
    this.#networkSection = networkSection
    this.#settingsSection = settingsSection
    this.#filesTabSection = filesTabSection
    this.#interfacesTabSection = interfacesTabSection
    this.#generalTabSection = generalTabSection

    homeSection
      ?.querySelector('anth-fab')
      ?.addEventListener('click', () => void host?.refresh())
    terminalSection
      ?.querySelector('[data-open-terminal]')
      ?.addEventListener('click', () => void openTerminalWindow())
    filesPanel.addEventListener(TAB_CHANGE_EVENT, (event) => {
      const { id } = (event as CustomEvent<TabChangeDetail>).detail
      // The hash stays the source of truth, so back and forward walk panels.
      location.hash = hashForPanel(id as PanelId)
    })
    networkPanel.addEventListener(TAB_CHANGE_EVENT, (event) => {
      const { id } = (event as CustomEvent<TabChangeDetail>).detail
      location.hash = hashForPanel(id as PanelId)
    })
    settingsPanel.addEventListener(TAB_CHANGE_EVENT, (event) => {
      const { id } = (event as CustomEvent<TabChangeDetail>).detail
      location.hash = hashForPanel(id as PanelId)
    })
    this.#shellMounted = true
  }

  /** Closes every panel stream when the shell stops being visible. */
  #unmountShell(container: HTMLElement): void {
    if (!this.#shellMounted) {
      return
    }
    this.#shellMounted = false
    this.#tabPanel = undefined
    this.#filesPanel = undefined
    this.#networkPanel = undefined
    this.#settingsPanel = undefined
    this.#homeSection = undefined
    this.#filesSection = undefined
    this.#terminalSection = undefined
    this.#processesSection = undefined
    this.#networkSection = undefined
    this.#settingsSection = undefined
    this.#filesTabSection = undefined
    this.#interfacesTabSection = undefined
    this.#generalTabSection = undefined
    container.innerHTML = ''
  }

  #paintTabs(): void {
    if (!this.#tabPanel) {
      return
    }
    this.#homeSection?.setAttribute('label', msg('Home'))
    this.#filesSection?.setAttribute('label', msg('Files'))
    this.#terminalSection?.setAttribute('label', msg('Terminal'))
    this.#processesSection?.setAttribute('label', msg('Processes'))
    this.#networkSection?.setAttribute('label', msg('Network'))
    this.#settingsSection?.setAttribute('label', msg('Settings'))
    this.#filesTabSection?.setAttribute('label', msg('Files'))
    this.#interfacesTabSection?.setAttribute('label', msg('Interfaces'))
    this.#generalTabSection?.setAttribute('label', msg('General'))
    const card = this.#homeSection?.querySelector('[data-host-card]')
    const label = card?.querySelector('.card-head .label')
    if (label) {
      label.textContent = msg('Host')
    }
    const refresh = card?.querySelector('anth-fab')
    if (refresh) {
      refresh.setAttribute('label', msg('Refresh host information'))
    }
    this.#tabPanel.active = this.#route.section
    if (this.#filesPanel) {
      this.#filesPanel.active = 'files'
    }
    if (this.#networkPanel) {
      this.#networkPanel.active = this.#route.section === 'network' ? this.#route.panel : 'interfaces'
    }
    if (this.#settingsPanel) {
      this.#settingsPanel.active = 'general'
    }
    this.#tabPanel.refresh()
    this.#filesPanel?.refresh()
    this.#networkPanel?.refresh()
    this.#settingsPanel?.refresh()
  }

  readonly #onHashChange = (): void => {
    const next = routeFrom(location.hash)
    if (next.section === this.#route.section && next.panel === this.#route.panel) {
      return
    }
    this.#route = next
    this.#paintNav()
    this.#paintTabs()
  }

  readonly #onLocaleChange = (): void => {
    this.#locale = currentLocale()
    this.render()
  }

  async #switchLocale(locale: PanelLocale): Promise<void> {
    await changeLocale(locale)
    this.#locale = locale
    this.render()
  }

  readonly #onLogin = (event: Event): void => {
    event.preventDefault()
    const form = event.currentTarget as HTMLFormElement
    const data = new FormData(form)
    const username = String(data.get('username') ?? '')
    const password = String(data.get('password') ?? '')
    void this.#perform(() => AnthApp.authClient!.login(username, password))
  }

  readonly #onRegister = (event: Event): void => {
    event.preventDefault()
    const form = event.currentTarget as HTMLFormElement
    const data = new FormData(form)
    const password = String(data.get('password') ?? '')
    const confirm = String(data.get('confirm') ?? '')
    if (password !== confirm) {
      this.#error = msg('The passwords do not match.')
      this.render()
      return
    }
    void this.#perform(() => AnthApp.authClient!.register(password))
  }

  async #logout(): Promise<void> {
    await this.#perform(async () => {
      closeTerminalWindow()
      await AnthApp.authClient!.logout()
    })
  }
}

/*
 * These rules only work together:
 *
 * - `height: 100dvh` with a `100svh` fallback plus `overflow: hidden` make the
 *   host the viewport rather than the document.
 * - `flex: 0 0 auto` on the header keeps its natural height.
 * - `min-height: 0` on the body and on `main` is what lets them shrink below
 *   their content. Without it the column grows, the document scrolls, and the
 *   header leaves with it — the exact bug this layout exists to prevent.
 * - `main` is the only scroll container.
 */
const STYLE = `
  :host { display: flex; flex-direction: column; height: 100svh; height: 100dvh;
          overflow: hidden; color: var(--text, #dbe6f2);
          background: transparent; box-sizing: border-box; }

  /* The author display rules below would otherwise beat the UA [hidden] rule,
     which is exactly what left the auth block taking the full column after
     login. */
  [hidden] { display: none !important; }

  /* While the login form is the whole page there is no navigation to show. */
  :host(.auth-only) .sidebar { display: none; }
  :host(.auth-only) .shell { grid-template-columns: minmax(0, 1fr); }
  :host(.auth-only) .menu { display: none; }
  :host(.auth-only) anth-nav-drawer { display: none; }

  .skip { position: absolute; top: -10rem; left: 0.5rem; z-index: 30; padding: 0.5rem 0.75rem;
          color: var(--text, #dbe6f2); border: 1px solid var(--accent, #7dd3fc);
          border-radius: 7px; background: var(--panel, #101721);
          font: 600 12px/1.3 var(--mono, monospace); text-decoration: none; }
  .skip:focus { top: 0.5rem; }
  @media (pointer: coarse) { .skip { display: inline-flex; align-items: center; min-height: 44px; } }

  header { display: flex; flex: 0 0 auto; flex-wrap: wrap; gap: 1rem; align-items: center;
           justify-content: space-between; padding: 0.9rem clamp(1rem, 4vw, 2.5rem);
           padding-top: max(0.9rem, env(safe-area-inset-top));
           border-bottom: 1px solid var(--panel-border, #223041);
           background: rgba(16, 23, 33, 0.72); }

  .header-main { display: flex; align-items: center; gap: 0.6rem; min-width: 0; }

  h1 { margin: 0; font: 600 16px/1.2 var(--mono, monospace); letter-spacing: 0.02em; }
  .version { color: var(--muted, #8b9bad); font: 12px/1.4 var(--mono, monospace); }

  /* Language and sign-out close the sidebar and the drawer. The auto top
     margin is what pins them to the bottom edge of a short sidebar. */
  .nav-foot { display: grid; gap: 0.5rem; margin-top: auto; padding-top: 0.75rem;
              border-top: 1px solid var(--panel-border, #223041); }
  .nav-foot .locale {
    display: inline-flex; align-items: center; gap: 6px; min-height: 36px;
    padding: 0 10px; border-radius: 7px; box-sizing: border-box;
    color: var(--text, #dbe6f2); border: 1px solid var(--panel-border, #223041);
    background: var(--widget-bg, rgba(16, 23, 33, 0.5)); }
  .nav-foot .locale select {
    flex: 1 1 auto; min-width: 0; margin: 0; padding: 0; border: 0; color: inherit;
    background: transparent; font: 12px/1.4 var(--mono, monospace); cursor: pointer; }
  .nav-foot .locale select:focus-visible, .nav-foot .locale:focus-within {
    outline: 2px solid var(--accent, #7dd3fc); outline-offset: 2px; }
  .nav-foot button {
    display: inline-flex; align-items: center; justify-content: center; gap: 6px;
    min-height: 36px; padding: 4px 10px; border-radius: 7px; cursor: pointer;
    font: 12px/1.4 var(--mono, monospace); color: var(--text, #dbe6f2);
    border: 1px solid var(--panel-border, #223041);
    background: var(--widget-bg, rgba(16, 23, 33, 0.5)); }
  .nav-foot button:hover { border-color: var(--accent, #7dd3fc); }
  .nav-foot button:disabled { opacity: 0.55; cursor: progress; }
  .nav-foot button:focus-visible { outline: 2px solid var(--accent, #7dd3fc); outline-offset: 2px; }
  /* The drawer is the narrow-layout sidebar, so its labels stay readable. */
  .nav-foot .btn-label { display: inline; }
  .nav-foot anth-icon, .auth-card anth-icon, .retry anth-icon { font-size: 14px; }
  .menu:focus-visible { outline: 2px solid var(--accent, #7dd3fc); outline-offset: 2px; }

  .menu { display: none; place-items: center; width: 44px; height: 44px; padding: 0;
          color: inherit; border: 1px solid var(--panel-border, #223041); border-radius: 9px;
          background: transparent; font: 400 18px/1 var(--sans, sans-serif); cursor: pointer; }
  .menu:hover { border-color: var(--accent, #7dd3fc); }

  .shell { display: grid; grid-template-columns: clamp(200px, 17vw, 240px) minmax(0, 1fr);
           flex: 1 1 auto; min-height: 0; overflow: hidden; }

  .sidebar { display: flex; flex-direction: column; align-self: stretch; min-height: 0;
             box-sizing: border-box; overflow-y: auto; overscroll-behavior: contain; padding: 1.1rem 0.75rem;
             padding-left: max(0.75rem, env(safe-area-inset-left));
             padding-bottom: max(0.75rem, env(safe-area-inset-bottom));
             border-right: 1px solid var(--panel-border, #223041);
             background: rgba(12, 17, 24, 0.72); }

  .sidebar-toggle {
    display: grid;
    flex: 0 0 auto;
    place-items: center;
    align-self: flex-end;
    width: 32px;
    height: 32px;
    margin-bottom: 0.6rem;
    padding: 0;
    color: var(--muted, #8b9bad);
    border: 1px solid transparent;
    border-radius: 7px;
    background: transparent;
    cursor: pointer;
    font: inherit;
  }
  .sidebar-toggle:hover { color: var(--text, #dbe6f2); border-color: var(--panel-border, #223041); }
  .sidebar-toggle:focus-visible { outline: 2px solid var(--accent, #7dd3fc); outline-offset: 2px; }
  .sidebar-toggle anth-icon { font-size: 16px; }

  .sidebar nav, anth-nav-drawer nav { display: grid; gap: 0.25rem; }
  .sidebar nav { flex: 0 0 auto; }

  .nav-item { display: flex; align-items: center; gap: 0.6rem; padding: 0.5rem 0.65rem;
              color: var(--muted, #8b9bad); text-decoration: none;
              border-left: 2px solid transparent; border-radius: 0 8px 8px 0;
              font: 600 12px/1.3 var(--mono, monospace); letter-spacing: 0.02em; }
  .nav-item:hover, .nav-item:focus-visible { color: var(--text, #dbe6f2);
              background: rgba(125, 211, 252, 0.06); outline: none; }
  .nav-item:focus-visible { box-shadow: inset 0 0 0 1px rgba(125, 211, 252, 0.45); }
  .nav-item[data-active] { color: var(--accent, #7dd3fc); border-left-color: var(--accent, #7dd3fc);
              background: rgba(125, 211, 252, 0.1); }
  .nav-icon { display: inline-flex; align-items: center; justify-content: center;
              width: 1.2rem; font-size: 1.05rem; line-height: 1; }
  .nav-label { flex: 1; }

  /* Desktop-only icon rail. The drawer keeps its labels, so every selector is
     scoped through .sidebar and the grid column is desktop-only. */
  @media (min-width: 48.0625rem) {
    :host(.sidebar-collapsed) .shell { grid-template-columns: 4.5rem minmax(0, 1fr); }
    :host(.sidebar-collapsed) .sidebar { padding: 0.9rem 0.4rem; }
    :host(.sidebar-collapsed) .sidebar .sidebar-toggle { align-self: center; }
    :host(.sidebar-collapsed) .sidebar .nav-item {
      justify-content: center;
      gap: 0;
      padding: 0.55rem 0;
    }
    :host(.sidebar-collapsed) .sidebar .nav-icon { width: auto; font-size: 1.15rem; }
    :host(.sidebar-collapsed) .sidebar .nav-label { display: none; }
    :host(.sidebar-collapsed) .sidebar .nav-foot { justify-items: center; }
    :host(.sidebar-collapsed) .sidebar .nav-foot .locale {
      position: relative;
      width: 44px;
      height: 44px;
      min-height: 44px;
      padding: 0;
      justify-content: center;
    }
    :host(.sidebar-collapsed) .sidebar .nav-foot .locale select {
      position: absolute;
      inset: 0;
      width: 100%;
      height: 100%;
      opacity: 0;
      cursor: pointer;
    }
    :host(.sidebar-collapsed) .sidebar .nav-foot .btn-label { display: none; }
    :host(.sidebar-collapsed) .sidebar .nav-foot button {
      width: 44px;
      height: 44px;
      min-height: 44px;
      padding: 0;
    }
  }

  main { min-width: 0; min-height: 0; overflow-y: auto; overscroll-behavior: contain;
         -webkit-overflow-scrolling: touch; padding: clamp(1rem, 3vw, 2rem);
         padding-right: max(clamp(1rem, 3vw, 2rem), env(safe-area-inset-right));
         padding-bottom: max(1.5rem, env(safe-area-inset-bottom));
         padding-left: max(clamp(1rem, 3vw, 2rem), env(safe-area-inset-left)); }
  main:focus { outline: none; }
  main > anth-tab-panel { display: block; max-width: 940px; }

  .card { background: rgba(16, 23, 33, 0.86); border: 1px solid var(--panel-border, #223041);
          border-radius: var(--radius, 10px); padding: 1rem 1.15rem; }
  .card-head { display: flex; align-items: center; justify-content: space-between; margin-bottom: 0.8rem; }
  .label { font-size: 11px; letter-spacing: 0.06em; text-transform: uppercase; color: var(--muted, #8b9bad); }
  .terminal-launcher { display: grid; gap: 0.75rem; justify-items: start; max-width: 34rem; }
  .terminal-launcher h2 { margin: 0; font: 600 1.15rem/1.2 var(--mono, monospace); }
  .terminal-launcher p { margin: 0; }
  .launch-terminal {
    display: inline-flex;
    align-items: center;
    gap: 0.5rem;
    min-height: 40px;
    padding: 0.5rem 0.85rem;
    border: 1px solid var(--accent, #7dd3fc);
    border-radius: 7px;
    background: rgba(125, 211, 252, 0.12);
    color: var(--text, #dbe6f2);
    font: 13px/1.4 var(--mono, monospace);
    cursor: pointer;
  }
  .launch-terminal:hover { background: rgba(125, 211, 252, 0.2); }

  .auth { display: grid; place-items: center; min-height: 100%; }
  .auth-card { display: grid; gap: 0.7rem; width: min(100%, 380px); padding: 1.2rem;
               border: 1px solid var(--panel-border, #223041); border-radius: 10px;
               background: var(--panel, #101721); }
  .auth-card h2 { margin: 0; font: 600 1.3rem/1.2 var(--mono, monospace); }
  .auth-card p { margin: 0; }
  .auth-card label { display: grid; gap: 0.3rem; color: var(--muted, #8b9bad);
                     font: 12px/1.4 var(--mono, monospace); }
  .auth-card input { box-sizing: border-box; width: 100%; min-height: 40px; padding: 7px 10px;
                     color: var(--text, #dbe6f2); border: 1px solid var(--panel-border, #223041);
                     border-radius: 6px; background: #172231; font: 16px/1.4 var(--sans, sans-serif); }
  .auth-card input:focus-visible { outline: 2px solid var(--accent, #7dd3fc); outline-offset: 2px; }
  .auth-card button, .retry { display: inline-flex; align-items: center; justify-content: center;
                     gap: 6px; min-height: 40px; padding: 7px 12px; cursor: pointer;
                     color: var(--text, #dbe6f2); border: 1px solid var(--panel-border, #223041);
                     border-radius: 6px; background: rgba(125, 211, 252, 0.08);
                     font: 600 12px/1.4 var(--mono, monospace); }
  .auth-card button:hover:not(:disabled), .retry:hover { border-color: var(--accent, #7dd3fc); }
  .auth-card button:disabled { opacity: 0.55; cursor: progress; }
  .auth-card button:focus-visible, .retry:focus-visible {
                     outline: 2px solid var(--accent, #7dd3fc); outline-offset: 2px; }

  .muted { color: var(--muted, #8b9bad); }
  .empty { margin: 0; padding: 1.5rem 0.75rem; text-align: center; color: var(--muted, #8b9bad);
           border: 1px dashed var(--panel-border, #223041); border-radius: 8px; }
  .error { margin: 0 0 0.75rem; padding: 0.6rem 0.75rem; border-radius: 8px;
           border: 1px solid rgba(248, 113, 113, 0.4);
           background: rgba(248, 113, 113, 0.12); color: var(--err, #f87171); }

  /* The sidebar becomes a drawer by width, never by orientation: width is what
     decides whether two columns fit. */
  @media (max-width: 48rem) {
    .shell { grid-template-columns: minmax(0, 1fr); }
    .sidebar { display: none; }
    .menu { display: grid; }
    .btn-label { display: none; }
    .nav-foot .locale { min-height: 44px; }
    .nav-foot button { min-height: 44px; }
    .auth-card button:has(.btn-label),
    .retry:has(.btn-label) {
      width: 44px;
      height: 44px;
      padding: 0;
      justify-content: center;
    }
    header { gap: 0.5rem;
             padding: 0.7rem max(0.8rem, env(safe-area-inset-right)) 0.7rem max(0.8rem, env(safe-area-inset-left));
             padding-top: max(0.7rem, env(safe-area-inset-top)); }
    main { padding: 0.8rem;
           padding-right: max(0.8rem, env(safe-area-inset-right));
           padding-bottom: calc(5rem + env(safe-area-inset-bottom));
           padding-left: max(0.8rem, env(safe-area-inset-left)); }
  }

  @media (pointer: coarse) {
    .nav-item { min-height: 44px; }
    .header-actions select, .header-actions button { min-height: 44px; }
  }
`

if (!customElements.get('anth-app')) {
  customElements.define('anth-app', AnthApp)
}
