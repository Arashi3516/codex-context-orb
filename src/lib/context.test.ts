import { describe, expect, it } from 'vitest'
import { evaluateContext, resolvePinned, createHandoffTemplate, createReviewPrompt } from './context'
import { demoSessions, demoAssessment, DEMO_PRIMARY_ID, DEMO_BACKGROUND_ID } from './demo'

const now = 2_000_000
const sample = () => demoSessions('handoff', now)[0]

describe('post-compaction semantic integrity', () => {
  it('keeps lifecycle metadata alone unknown, even after many compactions', () => {
    expect(evaluateContext({ ...sample(), assessment: null, compactions: 100 }, now).level).toBe('unknown')
  })
  it('allows a coherent long session after eight compactions to continue', () => {
    expect(evaluateContext(demoSessions('healthy', now)[0], now).level).toBe('healthy')
  })
  it('does not use occupancy, token validity, or raw compaction count as a quality score', () => {
    for (const usedTokens of [0, 199_999, -1, NaN, Infinity, null]) {
      expect(evaluateContext({ ...sample(), usedTokens, compactions: 0 }, now).level).toBe('handoff')
    }
  })
  it('recommends handoff for recurring post-compaction execution harm with corroborating evidence', () => {
    const health = evaluateContext(sample(), now)
    expect(health.level).toBe('handoff')
    expect(health.actionable).toHaveLength(2)
  })
  it('requires repeated compaction evidence before recommending a new session', () => {
    for (const compactions_observed of [null, 0, 1]) {
      const s = sample(); s.assessment!.compactions_observed = compactions_observed
      expect(evaluateContext(s, now).level).toBe('watch')
    }
  })
  it('does not escalate one isolated issue to handoff', () => {
    const s = sample(); s.assessment!.signals.pop()
    expect(evaluateContext(s, now).level).toBe('watch')
  })
  it('does not infer degradation from a single occurrence or pre-compaction history', () => {
    for (const field of ['recurrence', 'after_compaction'] as const) {
      const s = sample()
      s.assessment!.signals.forEach(item => { if (field === 'recurrence') item.recurrence = 'once'; else item.after_compaction = false })
      expect(evaluateContext(s, now).level).toBe('watch')
    }
  })
  it('excludes resolved issues and things that do not affect the next step', () => {
    for (const resolved of [true, false]) {
      const s = sample()
      s.assessment!.signals.forEach(item => { if (resolved) item.status = 'resolved'; else item.affects_next_step = false })
      expect(evaluateContext(s, now).level).toBe('healthy')
    }
  })
  it('requires sufficient review coverage and strong evidence for handoff', () => {
    const s = sample(); s.assessment!.coverage = 'partial'
    expect(evaluateContext(s, now).level).toBe('watch')
    s.assessment!.coverage = 'sufficient'
    s.assessment!.signals.forEach(item => { item.confidence = 'medium' })
    expect(evaluateContext(s, now).level).toBe('watch')
  })
  it('does not call an incomplete review or uncertain open issue clear', () => {
    const s = sample(); s.assessment = demoAssessment('unknown', now)
    expect(evaluateContext(s, now).level).toBe('unknown')
    s.assessment = demoAssessment('handoff', now)
    s.assessment.signals.forEach(item => { item.confidence = 'low' })
    expect(evaluateContext(s, now).level).toBe('unknown')
  })
  it('requires distinct observations, not two labels on one failure', () => {
    const s = sample(); s.assessment!.signals[1].evidence = s.assessment!.signals[0].evidence
    expect(evaluateContext(s, now).level).toBe('watch')
    s.assessment!.signals[1].evidence = s.assessment!.signals[0].evidence.slice(0, 2)
    expect(evaluateContext(s, now).level).toBe('watch')
  })
  it('does not count repeated references as corroborating or correction evidence', () => {
    const s = sample(); const signal = s.assessment!.signals[0]
    signal.evidence = [signal.evidence[0], signal.evidence[0], signal.evidence[0]]
    expect(evaluateContext(s, now).level).toBe('watch')
  })
  it('does not use blank references to satisfy correction or corroboration evidence', () => {
    const s = sample(); const signal = s.assessment!.signals[0]
    signal.evidence = [...signal.evidence.slice(0, 2), { ref: ' ', note: '没有可定位的来源' }]
    expect(evaluateContext(s, now).level).toBe('watch')
    signal.evidence = [{ ref: ' ', note: '没有可定位的来源' }, { ref: '  ', note: '仍没有来源' }]
    s.assessment!.signals = [signal]
    expect(evaluateContext(s, now).level).toBe('unknown')
  })
  it('keeps distinct case-sensitive references while trimming surrounding whitespace', () => {
    const s = sample(); const signal = s.assessment!.signals[0]
    signal.evidence = ['src/Foo.ts:1', 'src/foo.ts:1', 'decision:latest'].map(ref => ({ ref, note: '独立的合成来源' }))
    expect(evaluateContext(s, now).level).toBe('handoff')
    signal.evidence[1].ref = ' src/Foo.ts:1 '
    expect(evaluateContext(s, now).level).toBe('watch')
  })
  it('rejects mismatched identities, invalid dates and stale reviews', () => {
    expect(evaluateContext(undefined, now).level).toBe('unknown')
    for (const stamp of [-1, NaN, now + 60_001, now - 1_200_001]) {
      const s = sample(); s.assessment!.reviewed_at_ms = stamp
      expect(evaluateContext(s, now).level).toBe('unknown')
    }
    const s = sample(); s.assessment!.session_id = 'other-session'
    expect(evaluateContext(s, now).level).toBe('unknown')
  })
  it('invalidates the previous assessment after any later hook, including Stop', () => {
    expect(evaluateContext({ ...sample(), turnId: 'next-turn' }, now).level).toBe('unknown')
    for (const lastEvent of ['SessionStart', 'UserPromptSubmit', 'PreCompact', 'PostCompact', 'Stop', 'Interrupt']) {
      expect(evaluateContext({ ...sample(), lastEvent, observedAt: now + 1 }, now + 1).level).toBe('unknown')
    }
    expect(evaluateContext({ ...sample(), lastEvent: 'Stop', observedAt: now }, now).level).toBe('handoff')
  })
  it('cannot revive a compacted assessment when Stop replaces the latest event', () => {
    const s = { ...sample(), lastEvent: 'PostCompact', observedAt: now + 1 }
    expect(evaluateContext(s, now + 2).level).toBe('unknown')
    s.lastEvent = 'Stop'; s.observedAt = now + 2
    expect(evaluateContext(s, now + 2).level).toBe('unknown')
    expect(createHandoffTemplate(s, now + 2)).not.toContain('保留用户主动保存入口')
  })
  it('keeps later activity unknown when either side lacks a turn identifier', () => {
    for (const [turnId, reviewedTurn] of [['new-turn', null], [null, 'reviewed-turn'], [null, null]]) {
      const s = { ...sample(), turnId, lastEvent: 'Stop', observedAt: now + 1 }
      s.assessment!.turn_id = reviewedTurn
      expect(evaluateContext(s, now + 1).level).toBe('unknown')
    }
    const s = sample(); s.turnId = null; s.assessment!.turn_id = null
    expect(evaluateContext(s, now).level).toBe('handoff')
  })
  it('does not trust an invalid hook observation time', () => {
    for (const observedAt of [-1, NaN, now + 60_001, 0.5]) {
      expect(evaluateContext({ ...sample(), observedAt, lastEvent: 'Stop' }, now).level).toBe('unknown')
    }
  })
  it('rejects invalid reviewed compaction history', () => {
    for (const compactions_observed of [-1, NaN, 2.5]) {
      const s = sample(); s.assessment!.compactions_observed = compactions_observed
      expect(evaluateContext(s, now).level).toBe('unknown')
    }
  })
})

describe('session binding and clean handoff', () => {
  it('keeps A pinned when B becomes more active', () => {
    const sessions = demoSessions('handoff', now)
    sessions[1].observedAt = now + 1_000; sessions.reverse()
    expect(resolvePinned(sessions, DEMO_PRIMARY_ID)?.id).toBe(DEMO_PRIMARY_ID)
    expect(resolvePinned(sessions, DEMO_BACKGROUND_ID)?.id).toBe(DEMO_BACKGROUND_ID)
    expect(resolvePinned(sessions, null)).toBeUndefined()
    expect(resolvePinned(sessions, 'removed')).toBeUndefined()
  })
  it('carries the current objective and questions without inventing confirmed facts', () => {
    const template = createHandoffTemplate(sample(), now)
    expect(template).toContain('保留用户主动保存入口')
    expect(template).toContain('[仅保留已验证结论')
    expect(template).toContain('不能当作已确认事实')
    expect(template).toContain('明确不再沿用的信息')
  })
  it('does not prefill a stale or mismatched review into a fresh session', () => {
    const template = createHandoffTemplate(sample(), now + 1_200_001)
    expect(template).not.toContain('保留用户主动保存入口')
    expect(template).toContain('待填写')
  })
  it('offers an explicit review request without pretending to run it', () => {
    expect(createReviewPrompt(DEMO_PRIMARY_ID)).toContain(DEMO_PRIMARY_ID)
    expect(createReviewPrompt(null)).toContain('不能取得时')
  })
})
