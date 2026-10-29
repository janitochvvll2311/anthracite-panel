export type FirewallAction = 'allow' | 'deny'
export type FirewallChain = 'input' | 'output'
export type FirewallProtocol = 'tcp' | 'udp'
export type FirewallRuleAction = 'allow' | 'deny' | 'custom'

/** One rule in the managed `inet anthracite` table. */
export interface FirewallRule {
  handle: number
  chain: FirewallChain
  action: FirewallRuleAction
  protocol?: FirewallProtocol
  port?: number
  /** User comment for a managed rule, or the raw nft comment for a custom one. */
  comment?: string
}

/** The presence and default policy of one managed base chain. */
export interface FirewallChainState {
  chain: FirewallChain
  initialized: boolean
  policy: FirewallAction
}

/** One point-in-time view of the managed table. */
export interface FirewallSnapshot {
  initialized: boolean
  control_enabled: boolean
  rules: FirewallRule[]
  chains: FirewallChainState[]
  ruleset: string
  timestamp_ms: number
}

export interface FirewallClient {
  get(): Promise<FirewallSnapshot>
  /** Resolves to the daemon message. */
  add(
    chain: FirewallChain,
    action: FirewallAction,
    protocol: FirewallProtocol,
    port: number,
    comment?: string,
  ): Promise<string>
  update(
    handle: number,
    chain: FirewallChain,
    action: FirewallAction,
    protocol: FirewallProtocol,
    port: number,
    comment?: string,
  ): Promise<string>
  remove(handle: number): Promise<string>
  flush(chain: FirewallChain): Promise<string>
  setPolicy(chain: FirewallChain, policy: FirewallAction): Promise<string>
}
