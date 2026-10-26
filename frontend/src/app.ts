import { LitElement, css, html, nothing } from 'lit'
import { customElement, state } from 'lit/decorators.js'
import { localized, msg } from '@lit/localize'

import './host-info'
import './settings'
import type { AuthClient, AuthState } from './auth-client'
import type { HostInfoClient } from './host-info-client'
import type { SettingsClient } from './settings-client'
import { changeLocale, currentLocale } from './i18n'
import type { PanelLocale } from './i18n'

type Route = 'home' | 'settings'

@customElement('anth-app')
@localized()
export class AnthApp extends LitElement {
  static authClient: AuthClient | undefined
  static hostClient: HostInfoClient | undefined
  static settingsClient: SettingsClient | undefined

  @state()
  private auth?: AuthState

  @state()
  private error = ''

  @state()
  private busy = false

  @state()
  private locale: PanelLocale = 'en'

  @state()
  private route: Route = 'home'

  private readonly onHashChange = () => {
    this.route = this.routeOf()
  }

  override connectedCallback(): void {
    super.connectedCallback()
    this.locale = currentLocale()
    this.route = this.routeOf()
    globalThis.addEventListener('hashchange', this.onHashChange)
    queueMicrotask(() => void this.load())
  }

  override disconnectedCallback(): void {
    globalThis.removeEventListener('hashchange', this.onHashChange)
    super.disconnectedCallback()
  }

  private routeOf(): Route {
    return globalThis.location?.hash === '#/settings' ? 'settings' : 'home'
  }

  private async load(): Promise<void> {
    const client = AnthApp.authClient
    if (!client) {
      this.error = 'No auth client was configured'
      return
    }
    this.busy = true
    try {
      this.auth = await client.state()
      this.error = ''
    } catch (error) {
      this.error = error instanceof Error ? error.message : String(error)
    } finally {
      this.busy = false
    }
  }

  private async login(event: Event): Promise<void> {
    event.preventDefault()
    const form = event.currentTarget as HTMLFormElement
    const data = new FormData(form)
    const username = String(data.get('username') ?? '')
    const password = String(data.get('password') ?? '')
    await this.perform(() => AnthApp.authClient!.login(username, password))
  }

  private async register(event: Event): Promise<void> {
    event.preventDefault()
    const form = event.currentTarget as HTMLFormElement
    const data = new FormData(form)
    const password = String(data.get('password') ?? '')
    const confirm = String(data.get('confirm') ?? '')
    if (password !== confirm) {
      this.error = msg('The passwords do not match.')
      return
    }
    await this.perform(() => AnthApp.authClient!.register(password))
  }

  private async logout(): Promise<void> {
    await this.perform(() => AnthApp.authClient!.logout())
  }

  private async perform(action: () => Promise<void>): Promise<void> {
    this.busy = true
    try {
      await action()
      this.error = ''
      await this.load()
    } catch (error) {
      this.error = error instanceof Error ? error.message : String(error)
    } finally {
      this.busy = false
    }
  }

  private async switchLocale(event: Event): Promise<void> {
    const locale = (event.currentTarget as HTMLSelectElement).value as PanelLocale
    await changeLocale(locale)
    this.locale = locale
  }

  private ready(): boolean {
    return !!this.auth && (!this.auth.required || this.auth.authenticated)
  }

  private renderForm() {
    if (!this.auth) return nothing
    if (this.auth.bootstrap === 'web' && !this.auth.registered) {
      return html`
        <form class="auth" @submit=${this.register}>
          <h2>${msg('Set the first password')}</h2>
          <p class="muted">${msg('The operator account is')} <strong>${this.auth.username}</strong>.</p>
          <label>${msg('Password')}<input name="password" type="password" minlength="12" required autocomplete="new-password" /></label>
          <label>${msg('Confirm password')}<input name="confirm" type="password" minlength="12" required autocomplete="new-password" /></label>
          <button type="submit" ?disabled=${this.busy}>${msg('Create password')}</button>
        </form>
      `
    }
    return html`
      <form class="auth" @submit=${this.login}>
        <h2>${msg('Sign in')}</h2>
        <label>${msg('Username')}<input name="username" .value=${this.auth.username} autocomplete="username" required /></label>
        <label>${msg('Password')}<input name="password" type="password" autocomplete="current-password" required /></label>
        <button type="submit" ?disabled=${this.busy}>${msg('Sign in')}</button>
      </form>
    `
  }

  override render() {
    return html`
      <header>
        <div>
          <span class="eyebrow">Anthracite Panel</span>
          <h1>${msg('Host control')}</h1>
        </div>
        ${this.ready()
          ? html`<nav aria-label=${msg('Sections')}>
              <a href="#/" aria-current=${this.route === 'home' ? 'page' : nothing}>${msg('Host')}</a>
              <a href="#/settings" aria-current=${this.route === 'settings' ? 'page' : nothing}>${msg('Settings')}</a>
            </nav>`
          : nothing}
        <div class="header-actions">
          <select aria-label=${msg('Language')} .value=${this.locale} @change=${this.switchLocale}>
            <option value="en">${msg('English')}</option>
            <option value="es">${msg('Spanish')}</option>
          </select>
          ${this.ready()
            ? html`<button type="button" ?disabled=${this.busy} @click=${this.logout}>${msg('Sign out')}</button>`
            : nothing}
        </div>
      </header>
      <main>
        ${this.error ? html`<div class="error" role="alert">${this.error}</div>` : nothing}
        ${!this.auth
          ? this.error
            ? html`<p class="muted"><button type="button" ?disabled=${this.busy} @click=${() => this.load()}>${msg('Retry')}</button></p>`
            : html`<p class="muted">${msg('Loading…')}</p>`
          : this.ready()
            ? this.route === 'settings'
              ? html`<anth-settings .client=${AnthApp.settingsClient}></anth-settings>`
              : html`
                  <section class="welcome">
                    <div>
                      <p class="eyebrow">${msg('Overview')}</p>
                      <h2>${msg('Host control')}</h2>
                      <p class="muted">${msg('Live host information from the daemon.')}</p>
                    </div>
                  </section>
                  <anth-host-info .client=${AnthApp.hostClient}></anth-host-info>
                `
            : this.renderForm()}
      </main>
    `
  }

  static styles = css`
    :host {
      display: block;
      min-height: 100dvh;
      color: var(--text, #dbe6f2);
      background: var(--page-bg, #070a0f);
      font: 15px/1.55 var(--sans, system-ui, sans-serif);
    }
    header {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      justify-content: space-between;
      gap: 12px;
      padding: 18px clamp(16px, 4vw, 40px);
      border-bottom: 1px solid var(--panel-border, #223041);
      background: rgba(16, 23, 33, .72);
    }
    .header-actions { display: flex; gap: 8px; align-items: center; }
    nav { display: flex; gap: 4px; }
    nav a {
      padding: 6px 10px;
      border-radius: 6px;
      color: var(--muted, #8b9bad);
      text-decoration: none;
      font: 13px/1.4 var(--mono, monospace);
    }
    nav a:hover { color: var(--text, #dbe6f2); }
    nav a[aria-current='page'] { background: #172231; color: var(--text, #dbe6f2); }
    h1, h2 { margin: 0; font-family: var(--mono, monospace); }
    h1 { font-size: 18px; }
    h2 { font-size: clamp(24px, 4vw, 34px); }
    .eyebrow { color: var(--muted, #8b9bad); font: 11px/1.4 var(--mono, monospace); text-transform: uppercase; letter-spacing: .08em; }
    main { max-width: 1000px; margin: 0 auto; padding: clamp(18px, 4vw, 40px); display: grid; gap: 14px; }
    .welcome { padding: 14px; border: 1px solid var(--panel-border, #223041); border-radius: 10px; background: var(--panel, #101721); }
    .muted { color: var(--muted, #8b9bad); }
    .error { padding: 10px 12px; border-radius: 7px; background: rgba(248, 113, 113, .12); color: #fecaca; }
    .auth {
      display: grid;
      gap: 10px;
      max-width: 360px;
      padding: 18px;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 10px;
      background: var(--panel, #101721);
    }
    .auth label { display: grid; gap: 4px; color: var(--muted, #8b9bad); font: 12px/1.4 var(--mono, monospace); }
    input, select, button {
      border: 1px solid var(--panel-border, #223041);
      border-radius: 6px;
      background: #172231;
      color: var(--text, #dbe6f2);
      padding: 8px 10px;
      font: inherit;
    }
    input:focus-visible, select:focus-visible, button:focus-visible { outline: 2px solid var(--accent, #7dd3fc); outline-offset: 2px; }
    button { cursor: pointer; }
    button:disabled { opacity: .45; cursor: default; }
  `
}
