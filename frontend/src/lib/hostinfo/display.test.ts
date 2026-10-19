import { describe, expect, it } from 'vitest'
import {
  formatKib,
  formatLoad,
  formatPercent,
  formatUptime,
  loadPercent,
  memoryUsedKib,
  swapUsedKib,
  usagePercent,
} from './display'

describe('host info display helpers', () => {
  it('formats uptime with only non-zero components', () => {
    expect(formatUptime(0)).toBe('0s')
    expect(formatUptime(59)).toBe('59s')
    expect(formatUptime(90_061)).toBe('1d 1h 1m 1s')
    expect(formatUptime(3_600)).toBe('1h')
  })

  it('formats kibibytes with the largest unit', () => {
    expect(formatKib(0)).toBe('0 KiB')
    expect(formatKib(512)).toBe('512 KiB')
    expect(formatKib(2048)).toBe('2.0 MiB')
    expect(formatKib(8_388_608)).toBe('8.0 GiB')
  })

  it('computes used memory and swap without going negative', () => {
    expect(
      memoryUsedKib({
        total_kib: 100,
        available_kib: 40,
        swap_total_kib: 20,
        swap_free_kib: 5,
      }),
    ).toBe(60)
    expect(
      memoryUsedKib({
        total_kib: 100,
        available_kib: 140,
        swap_total_kib: 20,
        swap_free_kib: 5,
      }),
    ).toBe(0)
    expect(
      swapUsedKib({
        total_kib: 100,
        available_kib: 40,
        swap_total_kib: 20,
        swap_free_kib: 5,
      }),
    ).toBe(15)
  })

  it('clamps percentages and load ratios', () => {
    expect(usagePercent(50, 100)).toBe(50)
    expect(usagePercent(-1, 100)).toBe(0)
    expect(usagePercent(120, 100)).toBe(100)
    expect(usagePercent(1, 0)).toBe(0)
    expect(loadPercent(2, 4)).toBe(50)
    expect(loadPercent(10, 4)).toBe(100)
    expect(loadPercent(1, 0)).toBe(0)
  })

  it('formats percentages and load values', () => {
    expect(formatPercent(45.25)).toBe('45.3%')
    expect(formatLoad(0.5)).toBe('0.50')
  })
})
