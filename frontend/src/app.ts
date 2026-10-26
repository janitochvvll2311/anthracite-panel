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
import { destinations, hashFor, idFromHash } from './destinations'
import type { AnthFab } from './fab'
import { escape } from './html'
import type { AnthHostInfo } from './host-info'
import type { HostInfoClient } from './host-info-client'
import { changeLocale, currentLocale } from './i18n'
import type { PanelLocale } from './i18n'
import { DRAWER_CLOSE_EVENT } from './nav-drawer'
import type { AnthNavDrawer } from './nav-drawer'
import type { AnthSettings } from './settings'
import type { SettingsClient } from './settings-client'
import { TAB_CHANGE_EVENT } from './tab-panel'
import type { AnthTabPanel, TabChangeDetail } from './tab-panel'

// The shell renders these, so it imports them: a test that mounts `anth-app`
// on its own must still get working custom elements.
import './fab'
import './host-info'
import './nav-drawer'
import './settings'
import './tab-panel'

const VERSION = '0.3.0'

export class AnthApp extends HTMLElement {
  /** Set before mounting by `main`, or replaced by a test. */
  static authClient: AuthClient | undefined
  static hostClient: HostInfoClient | undefined
  static settingsClient: SettingsClient | undefined

  #auth?: AuthState
  #error = ''
  #busy = false
  #route = idFromHash('')
  #locale: PanelLocale = 'en'

  #frameMounted = false
  #shellMounted = false
  #tabPanel?: AnthTabPanel
  #hostSection?: HTMLElement
  #settingsSection?: HTMLElement
  #drawer?: AnthNavDrawer

  connectedCallback(): void {
    if (!this.shadowRoot) {
      this.attachShadow({ mode: 'open' })
    }
    this.#locale = currentLocale()
    this.#route = idFromHash(location.hash)
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
      this.#error = describe(error)
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
      this.#error = describe(error)
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
    this.#paintHeader()
    this.#paintNav()

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
        ? `<p class="empty"><button class="retry" type="button" data-retry>${msg('Retry')}</button></p>`
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
                  aria-controls="app-navigation" aria-expanded="false">☰</button>
          <h1>Anthracite Panel</h1>
          <span class="version">v${VERSION}</span>
        </div>
        <div class="header-actions">
          <select data-locale aria-label="${msg('Language')}"></select>
          <button data-signout type="button" hidden>${msg('Sign out')}</button>
        </div>
      </header>
      <div class="shell">
        <aside class="sidebar"><nav aria-label="${msg('Sections')}" data-sidebar></nav></aside>
        <main id="main" tabindex="-1">
          <p class="error" data-error role="alert" hidden></p>
          <div class="auth" data-auth></div>
          <div data-shell hidden></div>
        </main>
      </div>
      <anth-nav-drawer id="app-navigation"><nav aria-label="${msg('Sections')}" data-drawer></nav></anth-nav-drawer>
    `
    this.#frameMounted = true
    this.#drawer = root.querySelector('anth-nav-drawer') as AnthNavDrawer | null ?? undefined
    this.#wireFrame()
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
    this.#drawer?.addEventListener(DRAWER_CLOSE_EVENT, () => this.#setDrawer(false))
    root.querySelector('[data-signout]')?.addEventListener('click', () => void this.#logout())
    root.querySelector('[data-locale]')?.addEventListener('change', (event) => {
      void this.#switchLocale(event)
    })
  }

  #setDrawer(open: boolean): void {
    if (this.#drawer) {
      this.#drawer.open = open
    }
    this.shadowRoot?.querySelector('.menu')?.setAttribute('aria-expanded', String(open))
  }

  #paintHeader(): void {
    const root = this.shadowRoot
    if (!root) {
      return
    }
    const select = root.querySelector<HTMLSelectElement>('[data-locale]')
    if (select && select.options.length === 0) {
      select.innerHTML = `
        <option value="en">${msg('English')}</option>
        <option value="es">${msg('Spanish')}</option>
      `
    }
    if (select) {
      select.value = this.#locale
    }
    const signout = root.querySelector<HTMLButtonElement>('[data-signout]')
    if (signout) {
      signout.textContent = msg('Sign out')
      signout.toggleAttribute('hidden', !this.ready())
      signout.disabled = this.#busy
    }
  }

  /** One set of links, rendered twice: sidebar and drawer. */
  #paintNav(): void {
    const root = this.shadowRoot
    if (!root) {
      return
    }
    const links = destinations()
      .map(
        (destination) => `
          <a class="nav-item" href="${hashFor(destination)}" data-destination="${destination.id}"
             ${destination.id === this.#route ? 'data-active aria-current="page"' : ''}>
            <span class="nav-icon" aria-hidden="true">${escape(destination.icon)}</span>
            <span class="nav-label">${escape(destination.label)}</span>
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
          <button type="submit" ${this.#busy ? 'disabled' : ''}>${msg('Create password')}</button>
        </form>`
      : `
        <form class="auth-card" data-login>
          <h2>${msg('Sign in')}</h2>
          <label>${msg('Username')}<input name="username" value="${escape(auth.username)}" autocomplete="username" required></label>
          <label>${msg('Password')}<input name="password" type="password" autocomplete="current-password" required></label>
          <button type="submit" ${this.#busy ? 'disabled' : ''}>${msg('Sign in')}</button>
        </form>`
    container
      .querySelector('form')
      ?.addEventListener('submit', register ? this.#onRegister : this.#onLogin)
  }

  /** Mounts the tab strip and both panels the first time the shell is ready. */
  #ensureShell(container: HTMLElement): void {
    if (this.#shellMounted) {
      return
    }
    const panel = document.createElement('anth-tab-panel') as AnthTabPanel
    panel.setAttribute('active', this.#route)
    panel.innerHTML = `
      <section data-tab="host" label="${escape(msg('Host'))}">
        <section class="card" data-host-card>
          <div class="card-head">
            <span class="label">${msg('Host')}</span>
            <button type="button" data-refresh>${msg('Refresh')}</button>
          </div>
          <anth-host-info></anth-host-info>
          <anth-fab icon="↻" label="${escape(msg('Refresh host information'))}"></anth-fab>
        </section>
      </section>
      <section data-tab="settings" label="${escape(msg('Settings'))}">
        <anth-settings></anth-settings>
      </section>
    `
    // Assign the clients before the panel joins the page: the panels start
    // reading from `connectedCallback`, so a later assignment would be a card
    // that never loads.
    const host = panel.querySelector<AnthHostInfo>('anth-host-info')
    if (host) {
      host.client = AnthApp.hostClient
    }
    const settings = panel.querySelector<AnthSettings>('anth-settings')
    if (settings) {
      settings.client = AnthApp.settingsClient
    }
    container.append(panel)

    this.#tabPanel = panel
    this.#hostSection = panel.querySelector<HTMLElement>('[data-tab="host"]') ?? undefined
    this.#settingsSection = panel.querySelector<HTMLElement>('[data-tab="settings"]') ?? undefined
    panel
      .querySelector('[data-refresh]')
      ?.addEventListener('click', () => void host?.refresh())
    const fab = panel.querySelector<AnthFab>('anth-fab')
    fab?.addEventListener('click', () => void host?.refresh())
    panel.addEventListener(TAB_CHANGE_EVENT, (event) => {
      const { id } = (event as CustomEvent<TabChangeDetail>).detail
      // The hash stays the source of truth, so back and forward walk tabs.
      location.hash = `#/${id}`
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
    this.#hostSection = undefined
    this.#settingsSection = undefined
    container.innerHTML = ''
  }

  #paintTabs(): void {
    if (!this.#tabPanel) {
      return
    }
    this.#hostSection?.setAttribute('label', msg('Host'))
    this.#settingsSection?.setAttribute('label', msg('Settings'))
    const card = this.#hostSection?.querySelector('[data-host-card]')
    const label = card?.querySelector('.card-head .label')
    if (label) {
      label.textContent = msg('Host')
    }
    const refresh = card?.querySelector('[data-refresh]')
    if (refresh) {
      refresh.textContent = msg('Refresh')
    }
    card?.querySelector('anth-fab')?.setAttribute('label', msg('Refresh host information'))
    this.#tabPanel.active = this.#route
    this.#tabPanel.refresh()
  }

  readonly #onHashChange = (): void => {
    const next = idFromHash(location.hash)
    if (next === this.#route) {
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

  async #switchLocale(event: Event): Promise<void> {
    const locale = (event.currentTarget as HTMLSelectElement).value as PanelLocale
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
    await this.#perform(() => AnthApp.authClient!.logout())
  }
}

function describe(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
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

  .header-actions { display: flex; flex-wrap: wrap; gap: 0.5rem; align-items: center; }
  .header-actions select, .header-actions button {
    min-height: 36px; padding: 4px 10px; border-radius: 7px; font: 12px/1.4 var(--mono, monospace);
    color: var(--text, #dbe6f2); border: 1px solid var(--panel-border, #223041);
    background: var(--widget-bg, rgba(16, 23, 33, 0.5)); }
  .header-actions button { cursor: pointer; }
  .header-actions button:hover { border-color: var(--accent, #7dd3fc); }
  .header-actions button:disabled { opacity: 0.55; cursor: progress; }
  .header-actions select:focus-visible, .header-actions button:focus-visible,
  .menu:focus-visible { outline: 2px solid var(--accent, #7dd3fc); outline-offset: 2px; }

  .menu { display: none; place-items: center; width: 44px; height: 44px; padding: 0;
          color: inherit; border: 1px solid var(--panel-border, #223041); border-radius: 9px;
          background: transparent; font: 400 18px/1 var(--sans, sans-serif); cursor: pointer; }
  .menu:hover { border-color: var(--accent, #7dd3fc); }

  .shell { display: grid; grid-template-columns: clamp(200px, 17vw, 240px) minmax(0, 1fr);
           flex: 1 1 auto; min-height: 0; overflow: hidden; }

  .sidebar { align-self: stretch; height: 100%; min-height: 0; overflow-y: auto;
             overscroll-behavior: contain; padding: 1.1rem 0.75rem;
             padding-left: max(0.75rem, env(safe-area-inset-left));
             border-right: 1px solid var(--panel-border, #223041);
             background: rgba(12, 17, 24, 0.72); }

  .sidebar nav, anth-nav-drawer nav { display: grid; gap: 0.25rem; }

  .nav-item { display: flex; align-items: center; gap: 0.6rem; padding: 0.5rem 0.65rem;
              color: var(--muted, #8b9bad); text-decoration: none;
              border-left: 2px solid transparent; border-radius: 0 8px 8px 0;
              font: 600 12px/1.3 var(--mono, monospace); letter-spacing: 0.02em; }
  .nav-item:hover, .nav-item:focus-visible { color: var(--text, #dbe6f2);
              background: rgba(125, 211, 252, 0.06); outline: none; }
  .nav-item:focus-visible { box-shadow: inset 0 0 0 1px rgba(125, 211, 252, 0.45); }
  .nav-item[data-active] { color: var(--accent, #7dd3fc); border-left-color: var(--accent, #7dd3fc);
              background: rgba(125, 211, 252, 0.1); }
  .nav-icon { width: 1.2rem; text-align: center; font-size: 0.95rem; line-height: 1; }
  .nav-label { flex: 1; }

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
  button[data-refresh] { min-height: 36px; padding: 4px 12px; border-radius: 6px;
          color: var(--text, #dbe6f2); border: 1px solid var(--panel-border, #223041);
          background: rgba(125, 211, 252, 0.08); font: 600 12px/1.4 var(--mono, monospace);
          cursor: pointer; }
  button[data-refresh]:hover { border-color: var(--accent, #7dd3fc); }
  @media (pointer: coarse) { button[data-refresh] { min-height: 44px; font-size: max(16px, 1rem); } }

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
  .auth-card button, .retry { min-height: 40px; padding: 7px 12px; cursor: pointer;
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
