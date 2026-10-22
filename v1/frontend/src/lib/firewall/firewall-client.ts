/**
 * HTTP client for the managed nftables firewall endpoints.
 *
 * The component layer never talks to fetch directly, so tests and alternative
 * backends can inject their own implementation.
 */

export type FirewallAction = 'allow' | 'deny'
export type FirewallProtocol = 'tcp' | 'udp'

export interface FirewallRule {
  handle: number
  chain: string
  /** `allow`, `deny`, or `custom` for rules not created by the panel. */
  action: string
  protocol: FirewallProtocol | null
  port: number | null
  comment: string | null
}

export interface FirewallSnapshot {
  initialized: boolean
  control_enabled: boolean
  rules: FirewallRule[]
  ruleset: string
  /** Number of managed rules before any truncation. */
  total: number
  /** Whether `rules` or `ruleset` was shortened by the daemon. */
  truncated: boolean
  /** Human-readable notice when the snapshot was shortened. */
  warning?: string | null
}

export interface FirewallRuleInput {
  action: FirewallAction
  port: number
  protocol: FirewallProtocol
  comment?: string
}

export interface FirewallClient {
  get: () => Promise<FirewallSnapshot>
  addRule: (rule: FirewallRuleInput) => Promise<string>
  deleteRule: (handle: number) => Promise<string>
  flush: () => Promise<string>
}

export interface HttpFirewallClientOptions {
  baseUrl?: string
  fetchImpl?: typeof fetch
}

export class HttpFirewallClient implements FirewallClient {
  private readonly baseUrl: string
  private readonly fetchImpl: typeof fetch

  constructor(options: HttpFirewallClientOptions = {}) {
    this.baseUrl = options.baseUrl ?? '/api/firewall'
    this.fetchImpl = options.fetchImpl ?? globalThis.fetch.bind(globalThis)
  }

  async get(): Promise<FirewallSnapshot> {
    const response = await this.fetchImpl(this.baseUrl, {
      headers: { accept: 'application/json' },
    })
    if (!response.ok) throw await responseError(response)
    return (await response.json()) as FirewallSnapshot
  }

  async addRule(rule: FirewallRuleInput): Promise<string> {
    return this.post('/rules', {
      action: rule.action,
      port: rule.port,
      protocol: rule.protocol,
      comment: rule.comment,
    })
  }

  async deleteRule(handle: number): Promise<string> {
    return this.post(`/rules/${handle}/delete`)
  }

  async flush(): Promise<string> {
    return this.post('/flush')
  }

  private async post(path: string, body?: unknown): Promise<string> {
    const response = await this.fetchImpl(`${this.baseUrl}${path}`, {
      method: 'POST',
      headers: {
        accept: 'application/json',
        ...(body === undefined ? {} : { 'content-type': 'application/json' }),
      },
      body: body === undefined ? undefined : JSON.stringify(body),
    })
    if (!response.ok) throw await responseError(response)
    const value = (await response.json()) as { message?: unknown }
    return typeof value.message === 'string' ? value.message : 'done'
  }
}

async function responseError(response: Response): Promise<Error> {
  let detail = `${response.status} ${response.statusText}`.trim()

  try {
    const body = (await response.json()) as { error?: unknown }
    if (typeof body.error === 'string') detail = body.error
  } catch {
    // The body was not JSON; keep the status text.
  }

  return new Error(detail)
}
