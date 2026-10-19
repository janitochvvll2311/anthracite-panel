import type { FirewallRule } from './firewall-client'

/** Parses the port input used by the add-rule form. */
export function parsePort(value: string): number | null {
  const trimmed = value.trim()
  if (!/^\d+$/.test(trimmed)) return null
  const port = Number(trimmed)
  return Number.isSafeInteger(port) && port >= 1 && port <= 65_535
    ? port
    : null
}

export function actionLabel(action: string): string {
  switch (action) {
    case 'allow':
      return 'ALLOW'
    case 'deny':
      return 'DENY'
    default:
      return 'CUSTOM'
  }
}

export function ruleTarget(rule: FirewallRule): string {
  if (rule.protocol === null || rule.port === null) return '—'
  return `${rule.protocol}/${rule.port}`
}

export function ruleComment(rule: FirewallRule): string {
  return rule.comment === null || rule.comment === '' ? '—' : rule.comment
}
