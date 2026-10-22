import { vi } from 'vitest'
import type {
  FirewallAction,
  FirewallClient,
  FirewallProtocol,
  FirewallRule,
  FirewallRuleInput,
  FirewallSnapshot,
} from '../lib/firewall/firewall-client'

export function firewallRule(
  overrides: Partial<FirewallRule> = {},
): FirewallRule {
  return {
    handle: 3,
    chain: 'input',
    action: 'allow',
    protocol: 'tcp',
    port: 22,
    comment: 'ssh',
    ...overrides,
  }
}

export function firewallSnapshot(
  overrides: Partial<FirewallSnapshot> = {},
): FirewallSnapshot {
  return {
    initialized: true,
    control_enabled: true,
    rules: [firewallRule()],
    ruleset: 'table inet anthracite { chain input { ... } }',
    total: 1,
    truncated: false,
    ...overrides,
  }
}

export class FakeFirewallClient implements FirewallClient {
  current = firewallSnapshot()
  getError?: Error
  addError?: Error
  deleteError?: Error
  flushError?: Error
  addCalls: FirewallRuleInput[] = []
  deleteCalls: number[] = []
  flushCalls = 0

  get = vi.fn(async (): Promise<FirewallSnapshot> => {
    if (this.getError) throw this.getError
    return this.current
  })

  addRule = vi.fn(
    async (rule: FirewallRuleInput): Promise<string> => {
      if (this.addError) throw this.addError
      this.addCalls.push(rule)
      const verb = rule.action === 'allow' ? 'allowed' : 'denied'
      return `${verb} ${rule.protocol}/${rule.port}`
    },
  )

  deleteRule = vi.fn(async (handle: number): Promise<string> => {
    if (this.deleteError) throw this.deleteError
    this.deleteCalls.push(handle)
    return `deleted firewall rule ${handle}`
  })

  flush = vi.fn(async (): Promise<string> => {
    if (this.flushError) throw this.flushError
    this.flushCalls += 1
    return 'flushed 1 firewall rule'
  })

  emit(snapshot: FirewallSnapshot): void {
    this.current = snapshot
  }
}

export type { FirewallAction, FirewallProtocol }
