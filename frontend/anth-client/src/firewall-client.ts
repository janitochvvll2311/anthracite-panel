import type {
  Change,
  FirewallAction,
  FirewallChain,
  FirewallClient,
  FirewallProtocol,
  FirewallSnapshot,
} from 'anth-wire'

import {
  HttpTransport,
  asTransport,
  defaultTransport,
} from './http'

export class HttpFirewallClient implements FirewallClient {
  private readonly transport: HttpTransport

  constructor(transport: HttpTransport | typeof fetch = defaultTransport) {
    this.transport = asTransport(transport)
  }

  get(): Promise<FirewallSnapshot> {
    return this.transport.json<FirewallSnapshot>('GET', '/api/firewall')
  }

  add(
    chain: FirewallChain,
    action: FirewallAction,
    protocol: FirewallProtocol,
    port: number,
    comment?: string,
  ): Promise<Change> {
    return this.transport.json<Change>('POST', '/api/firewall/rule', {
      chain,
      action,
      protocol,
      port,
      ...(comment ? { comment } : {}),
    })
  }

  update(
    handle: number,
    chain: FirewallChain,
    action: FirewallAction,
    protocol: FirewallProtocol,
    port: number,
    comment?: string,
  ): Promise<Change> {
    return this.transport.json<Change>('POST', '/api/firewall/update', {
      handle,
      chain,
      action,
      protocol,
      port,
      ...(comment ? { comment } : {}),
    })
  }

  remove(handle: number): Promise<Change> {
    return this.transport.json<Change>('POST', '/api/firewall/delete', {
      handle,
    })
  }

  flush(chain: FirewallChain): Promise<Change> {
    return this.transport.json<Change>('POST', '/api/firewall/flush', {
      chain,
    })
  }

  setPolicy(chain: FirewallChain, policy: FirewallAction): Promise<Change> {
    return this.transport.json<Change>('POST', '/api/firewall/policy', {
      chain,
      policy,
    })
  }
}
