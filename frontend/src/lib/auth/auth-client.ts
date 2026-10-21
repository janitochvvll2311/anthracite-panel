/**
 * Panel authentication client.
 *
 * The tokens live in `HttpOnly` cookies, so this module never sees one and has
 * nothing to store. It only knows three things: whether the session is open,
 * how to start one, and how to refresh it before the access token expires.
 *
 * A single-flight refresh is deliberate. Every panel watching a stream would
 * otherwise refresh on its own, and because rotation invalidates the presented
 * token, the second refresh would fail and log the operator out. The shared
 * promise means the whole page rotates once.
 */

import { responseError } from '../http'

export interface SessionInfo {
  enabled: boolean
  authenticated: boolean
  username: string
  access_expires_in: number
  refresh_expires_in: number
}

export interface AuthClientOptions {
  baseUrl?: string
  fetchImpl?: typeof fetch
  /** Injectable clock, so tests do not wait for a real timer. */
  setTimeoutImpl?: typeof setTimeout
}

/** Refresh this many seconds before the access token expires. */
const REFRESH_LEAD_SECONDS = 30
/** Never schedule a refresh closer than this, or a short TTL spins. */
const MIN_DELAY_MS = 1_000

export class AuthClient {
  private readonly baseUrl: string
  private readonly fetchImpl: typeof fetch
  private readonly setTimeoutImpl: typeof setTimeout

  private refreshTimer?: ReturnType<typeof setTimeout>
  private inFlightRefresh?: Promise<SessionInfo>
  /** Called when a refresh fails for good, so the shell can show the login. */
  private expiredHandler?: () => void

  constructor(options: AuthClientOptions = {}) {
    this.baseUrl = options.baseUrl ?? ''
    this.fetchImpl =
      options.fetchImpl ?? ((input, init) => globalThis.fetch(input, init))
    this.setTimeoutImpl =
      options.setTimeoutImpl ??
      ((handler, timeout) => globalThis.setTimeout(handler, timeout))
  }

  /** Registers the callback that runs when the session cannot be renewed. */
  onExpired(handler: () => void): void {
    this.expiredHandler = handler
  }

  async session(): Promise<SessionInfo> {
    const response = await this.fetchImpl(`${this.baseUrl}/api/auth/session`, {
      credentials: 'same-origin',
    })
    if (!response.ok) throw await responseError(response)
    return (await response.json()) as SessionInfo
  }

  async login(username: string, password: string): Promise<SessionInfo> {
    const response = await this.fetchImpl(`${this.baseUrl}/api/auth/login`, {
      method: 'POST',
      credentials: 'same-origin',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ username, password }),
    })
    if (!response.ok) throw await responseError(response)

    const session = await this.session()
    this.scheduleRefresh(session)
    return session
  }

  async logout(): Promise<void> {
    this.cancelRefresh()
    try {
      await this.fetchImpl(`${this.baseUrl}/api/auth/logout`, {
        method: 'POST',
        credentials: 'same-origin',
      })
    } catch {
      // A failed logout still ends the local session; the cookie expires on its
      // own and the operator is back at the login screen either way.
    }
  }

  async changePassword(
    currentPassword: string,
    newPassword: string,
  ): Promise<void> {
    const response = await this.fetchImpl(`${this.baseUrl}/api/auth/password`, {
      method: 'POST',
      credentials: 'same-origin',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        current_password: currentPassword,
        new_password: newPassword,
      }),
    })
    if (!response.ok) throw await responseError(response)
    this.cancelRefresh()
  }

  /**
   * Refreshes the pair, sharing one request across concurrent callers.
   *
   * On failure the refresh token is gone, so there is nothing left to retry:
   * the handler fires and every panel stops asking.
   */
  refresh(): Promise<SessionInfo> {
    if (this.inFlightRefresh) return this.inFlightRefresh

    const request = (async () => {
      const response = await this.fetchImpl(`${this.baseUrl}/api/auth/refresh`, {
        method: 'POST',
        credentials: 'same-origin',
      })
      if (!response.ok) throw await responseError(response)

      const session = await this.session()
      this.scheduleRefresh(session)
      return session
    })()
      .catch((error: unknown) => {
        this.cancelRefresh()
        this.expiredHandler?.()
        throw error
      })
      .finally(() => {
        this.inFlightRefresh = undefined
      })

    this.inFlightRefresh = request
    return request
  }

  /** Arms the silent refresh, replacing any timer already pending. */
  scheduleRefresh(session: SessionInfo): void {
    this.cancelRefresh()
    if (!session.enabled || !session.authenticated) return

    const seconds = Math.max(session.access_expires_in - REFRESH_LEAD_SECONDS, 0)
    const delay = Math.max(seconds * 1000, MIN_DELAY_MS)
    this.refreshTimer = this.setTimeoutImpl(() => {
      void this.refresh().catch(() => undefined)
    }, delay)
  }

  cancelRefresh(): void {
    if (this.refreshTimer !== undefined) {
      clearTimeout(this.refreshTimer)
      this.refreshTimer = undefined
    }
  }
}
