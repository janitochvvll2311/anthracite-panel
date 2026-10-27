import { responseError } from './http'

export type FirewallAction = 'allow' | 'deny'
export type FirewallProtocol = 'tcp' | 'udp'
export type FirewallRuleAction = 'allow' | 'deny' | 'custom'

/** One rule in the managed `inet anthracite input` chain. */
export interface FirewallRule {
  handle: number
  chain: string
  action: FirewallRuleAction
  protocol?: FirewallProtocol
  port?: number
  /** User comment for a managed rule, or the raw nft comment for a custom one. */
  comment?: string
}

/** One point-in-time view of the managed table. */
export interface FirewallSnapshot {
  initialized: boolean
  control_enabled: boolean
  rules: FirewallRule[]
  ruleset: string
  timestamp_ms: number
}

export interface FirewallClient {
  get(): Promise<FirewallSnapshot>
  /** Resolves to the daemon message. */
  add(
    action: FirewallAction,
    protocol: FirewallProtocol,
    port: number,
    comment?: string,
  ): Promise<string>
  remove(handle: number): Promise<string>
  flush(): Promise<string>
}

export class HttpFirewallClient implements FirewallClient {
  private readonly fetchImpl: typeof fetch

  constructor(fetchImpl: typeof fetch = fetch) {
    // Same rule as the auth client: fetch must not be called as a method.
    this.fetchImpl = fetchImpl.bind(globalThis)
  }

  async get(): Promise<FirewallSnapshot> {
    const response = await this.fetchImpl('/api/firewall')
    if (!response.ok) throw await responseError(response)
    return (await response.json()) as FirewallSnapshot
  }

  async add(
    action: FirewallAction,
    protocol: FirewallProtocol,
    port: number,
    comment?: string,
  ): Promise<string> {
    return this.action('/api/firewall/rule', {
      action,
      protocol,
      port,
      ...(comment ? { comment } : {}),
    })
  }

  async remove(handle: number): Promise<string> {
    return this.action('/api/firewall/delete', { handle })
  }

  async flush(): Promise<string> {
    return this.action('/api/firewall/flush', {})
  }

  private async action(path: string, body: unknown): Promise<string> {
    const response = await this.fetchImpl(path, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    })
    if (!response.ok) throw await responseError(response)
    const result = (await response.json()) as { message?: string }
    return result.message ?? ''
  }
}
