import { responseError } from './http'
import type { FirewallAction, FirewallChain, FirewallProtocol, FirewallSnapshot, FirewallClient } from 'anth-wire'

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
    chain: FirewallChain,
    action: FirewallAction,
    protocol: FirewallProtocol,
    port: number,
    comment?: string,
  ): Promise<string> {
    return this.action('/api/firewall/rule', {
      chain,
      action,
      protocol,
      port,
      ...(comment ? { comment } : {}),
    })
  }

  async update(
    handle: number,
    chain: FirewallChain,
    action: FirewallAction,
    protocol: FirewallProtocol,
    port: number,
    comment?: string,
  ): Promise<string> {
    return this.action('/api/firewall/update', {
      handle,
      chain,
      action,
      protocol,
      port,
      ...(comment ? { comment } : {}),
    })
  }

  async remove(handle: number): Promise<string> {
    return this.action('/api/firewall/delete', { handle })
  }

  async flush(chain: FirewallChain): Promise<string> {
    return this.action('/api/firewall/flush', { chain })
  }

  async setPolicy(chain: FirewallChain, policy: FirewallAction): Promise<string> {
    return this.action('/api/firewall/policy', { chain, policy })
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
