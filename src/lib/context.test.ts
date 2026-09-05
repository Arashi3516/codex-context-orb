import { describe, expect, it } from 'vitest'
import { evaluateContext, resolvePinned, createHandoffTemplate } from './context'
import { demoSessions, DEMO_PRIMARY_ID, DEMO_BACKGROUND_ID } from './demo'

const now = 1_000_000
describe('evidence-based context health', () => {
  it('keeps missing occupancy unknown even when hook metadata exists', () => {
    const sample = demoSessions('unknown', now)[0]
    expect(evaluateContext(sample, now).level).toBe('unknown')
  })
  it('rejects missing, stale, future and impossible telemetry', () => {
    const sample = demoSessions('healthy', now)[0]
    expect(evaluateContext(undefined, now).level).toBe('unknown')
    expect(evaluateContext(sample, now + 300_001).level).toBe('unknown')
    expect(evaluateContext({ ...sample, observedAt: now + 60_001 }, now).level).toBe('unknown')
    for (const usedTokens of [-1, NaN, Infinity, 200_001]) {
      expect(evaluateContext({ ...sample, usedTokens }, now).level).toBe('unknown')
    }
  })
  it('warns at the actual threshold rather than a rounded display value', () => {
    const sample = { ...demoSessions('healthy', now)[0], usedTokens: 179_999 }
    expect(evaluateContext(sample, now).level).toBe('watch')
    expect(evaluateContext({ ...sample, usedTokens: 180_000 }, now).level).toBe('handoff')
  })
  it('does not declare long context semantically dirty', () => {
    const health = evaluateContext(demoSessions('handoff', now)[0], now)
    expect(health.level).toBe('handoff')
    expect(health.reasons.every(reason => !reason.title.includes('混乱'))).toBe(true)
  })
  it('treats repeated compaction as caution, not automatic handoff', () => {
    const sample = { ...demoSessions('healthy', now)[0], compactions: 3 }
    expect(evaluateContext(sample, now).level).toBe('watch')
  })
})

describe('explicit session binding', () => {
  it('keeps A pinned while B keeps receiving newer background events', () => {
    const sessions = demoSessions('healthy', now)
    sessions[1].observedAt = now + 1_000
    sessions.reverse()
    const current = resolvePinned(sessions, DEMO_PRIMARY_ID)
    expect(current?.id).toBe(DEMO_PRIMARY_ID)
    expect(evaluateContext(current, now).level).toBe('healthy')
    expect(resolvePinned(sessions, DEMO_BACKGROUND_ID)?.id).toBe(DEMO_BACKGROUND_ID)
  })
  it('does not select any session on disconnect or absent pin', () => {
    expect(resolvePinned(demoSessions('healthy', now), null)).toBeUndefined()
    expect(resolvePinned(demoSessions('healthy', now), 'removed')).toBeUndefined()
  })
  it('never invents verified facts in the handoff template', () => {
    const template = createHandoffTemplate(demoSessions('watch', now)[0])
    expect(template).toContain('待填写')
    expect(template).toContain('[仅保留已验证结论')
  })
})
