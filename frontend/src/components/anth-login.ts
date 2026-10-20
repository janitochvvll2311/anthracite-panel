/**
 * Login screen for the panel.
 *
 * The shell shows this instead of the feature pages when `GET /api/auth/session`
 * says the cookies are gone. It is a real form so Enter submits and a password
 * manager recognises the two fields; the component never stores the password and
 * clears it as soon as the request returns.
 *
 * The component reports success upward rather than navigating. The shell owns
 * the session lifecycle and remounts the route, so this element has no reason to
 * know how the app is routed.
 */

import { LitElement, css, html } from 'lit'
import { customElement, property, state } from 'lit/decorators.js'
import { localized, msg } from '@lit/localize'

import '../lib/auth/auth-client'
import type { AuthClient, SessionInfo } from '../lib/auth/auth-client'

@customElement('anth-login')
@localized()
export class AnthLogin extends LitElement {
  static override styles = css`
    :host {
      display: grid;
      place-items: center;
      padding: clamp(1.5rem, 6vw, 4rem);
    }

    form {
      display: grid;
      gap: 1rem;
      width: min(100%, 380px);
      padding: 1.6rem;
      border: 1px solid var(--panel-border, #223041);
      border-radius: 14px;
      background: rgba(16, 23, 33, 0.9);
    }

    h1 {
      margin: 0;
      font: 600 18px/1.3 var(--mono, monospace);
    }

    p.lede {
      margin: 0;
      color: var(--muted, #8b9bad);
      font-size: 13px;
    }

    label {
      display: grid;
      gap: 0.35rem;
      font: 600 11px/1.4 var(--mono, monospace);
      letter-spacing: 0.06em;
      text-transform: uppercase;
      color: var(--muted, #8b9bad);
    }

    input {
      padding: 0.55rem 0.65rem;
      color: inherit;
      font: 14px/1.4 var(--sans, system-ui, sans-serif);
      border: 1px solid var(--panel-border, #223041);
      border-radius: 8px;
      background: var(--panel, #101721);
    }

    input:focus-visible {
      outline: 2px solid rgba(125, 211, 252, 0.55);
      outline-offset: 1px;
    }

    button {
      padding: 0.6rem 0.8rem;
      color: #06121c;
      font: 700 13px/1.2 var(--mono, monospace);
      letter-spacing: 0.04em;
      border: 0;
      border-radius: 8px;
      background: var(--accent, #7dd3fc);
      cursor: pointer;
    }

    button:disabled {
      opacity: 0.6;
      cursor: progress;
    }

    .error {
      margin: 0;
      padding: 0.55rem 0.65rem;
      color: var(--err, #f87171);
      font-size: 13px;
      border: 1px solid rgba(248, 113, 113, 0.4);
      border-radius: 8px;
      background: rgba(248, 113, 113, 0.08);
    }
  `

  @property({ attribute: false })
  client?: AuthClient

  /** Called with the fresh session once the login succeeds. */
  @property({ attribute: false })
  onAuthenticated?: (session: SessionInfo) => void

  @state()
  private busy = false

  @state()
  private error = ''

  override render() {
    return html`
      <form @submit=${this.onSubmit} autocomplete="on">
        <h1>${msg('Sign in')}</h1>
        <p class="lede">
          ${msg('This panel controls the host. Sign in to continue.')}
        </p>
        <label>
          ${msg('User')}
          <input
            name="username"
            type="text"
            autocomplete="username"
            required
            .value=${this.username}
            ?disabled=${this.busy}
            @input=${this.onUsername}
          />
        </label>
        <label>
          ${msg('Password')}
          <input
            name="password"
            type="password"
            autocomplete="current-password"
            required
            .value=${this.password}
            ?disabled=${this.busy}
            @input=${this.onPassword}
          />
        </label>
        ${this.error
          ? html`<p class="error" role="alert">${this.error}</p>`
          : null}
        <button type="submit" ?disabled=${this.busy}>
          ${this.busy ? msg('Signing in…') : msg('Sign in')}
        </button>
      </form>
    `
  }

  private username = 'admin'
  private password = ''

  private onUsername = (event: Event): void => {
    this.username = (event.target as HTMLInputElement).value
  }

  private onPassword = (event: Event): void => {
    this.password = (event.target as HTMLInputElement).value
  }

  private async onSubmit(event: Event): Promise<void> {
    event.preventDefault()
    if (this.busy || !this.client) return

    this.busy = true
    this.error = ''
    try {
      const session = await this.client.login(this.username, this.password)
      this.password = ''
      this.onAuthenticated?.(session)
    } catch (error) {
      // The server answers one message for a wrong user and a wrong password,
      // so the screen cannot be used to enumerate accounts.
      this.error = error instanceof Error ? error.message : msg('Sign in failed')
      this.password = ''
    } finally {
      this.busy = false
    }
  }
}

declare global {
  interface HTMLElementTagNameMap {
    'anth-login': AnthLogin
  }
}
