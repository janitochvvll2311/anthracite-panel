import { describe, expect, it } from 'vitest'
import { actionLabel, parsePort, ruleComment, ruleTarget } from './display'
import { firewallRule } from '../../test/fake-firewall-client'

describe('firewall display helpers', () => {
  it('parses valid ports and rejects everything else', () => {
    expect(parsePort('22')).toBe(22)
    expect(parsePort(' 65535 ')).toBe(65535)
    expect(parsePort('0')).toBeNull()
    expect(parsePort('65536')).toBeNull()
    expect(parsePort('22/tcp')).toBeNull()
    expect(parsePort('')).toBeNull()
  })

  it('labels actions and rule targets', () => {
    expect(actionLabel('allow')).toBe('ALLOW')
    expect(actionLabel('deny')).toBe('DENY')
    expect(actionLabel('manual')).toBe('CUSTOM')
    expect(ruleTarget(firewallRule())).toBe('tcp/22')
    expect(
      ruleTarget(firewallRule({ protocol: null, port: null })),
    ).toBe('—')
  })

  it('shows a dash for an empty comment', () => {
    expect(ruleComment(firewallRule({ comment: null }))).toBe('—')
    expect(ruleComment(firewallRule({ comment: '' }))).toBe('—')
    expect(ruleComment(firewallRule({ comment: 'web' }))).toBe('web')
  })
})
