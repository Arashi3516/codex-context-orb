import { describe, expect, it } from 'vitest'
import { demoSessions } from './demo'
import { presentCapacity, presentCompactions, presentRisk } from './presentation'

const now = 2_000_000
describe('independent context indicators', () => {
  it('keeps high usage and repeated compactions separate from evidence risk', () => {
    const snapshot = demoSessions('passed', now)[0]
    expect(presentRisk(snapshot, now).tone).toBe('aligned')
    expect(presentCapacity(snapshot, now).percent).toBe(98)
    expect(presentCompactions(snapshot).value).toBe('8')
    expect(presentRisk({ ...snapshot, usedTokens: 0, compactions: 0 }, now).tone).toBe('aligned')
    expect(presentRisk(demoSessions('failed', now)[0], now).tone).toBe('deviation')
    expect(presentRisk(demoSessions('unknown', now)[0], now).tone).toBe('unknown')
  })
  it('does not promote copied token fields or one PostCompact hook into real counters', () => {
    const snapshot = { ...demoSessions('passed', now)[0], source: 'codex-hook' as const, lastEvent: 'PostCompact' }
    expect(presentCapacity(snapshot, now).ratio).toBeNull()
    expect(presentCompactions(snapshot)).toEqual({ value: '未记录', detail: '最近收到压缩完成事件，累计次数未知' })
  })
  it('rejects invalid demo counters and marks historical readings', () => {
    const snapshot = demoSessions('passed', now)[0]
    for (const usedTokens of [-1, Number.NaN, Infinity, 200_001, .2]) {
      expect(presentCapacity({ ...snapshot, usedTokens }, now).ratio).toBeNull()
    }
    expect(presentCapacity({ ...snapshot, windowTokens: 0 }, now).ratio).toBeNull()
    expect(presentCapacity(snapshot, now + 6 * 60_000).stale).toBe(true)
    expect(presentCapacity(undefined, now).percent).toBeNull()
    expect(presentCompactions(undefined).value).toBe('未记录')
  })
})
