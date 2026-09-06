import { describe, expect, it } from 'vitest'
import { demoSessions } from './demo'
import { captureOrbIntent, isCurrentOrbIntent, primaryOrbAction } from './orb-actions'

const now = 2_000_000
describe('capability-aware orb actions', () => {
  it('offers real preparation and inspection without inventing a host operation', () => {
    expect(primaryOrbAction(undefined, now).kind).toBe('select-session')
    const snapshot = demoSessions('passed', now)[0]
    expect(primaryOrbAction({ ...snapshot, report: undefined }, now).kind).toBe('prepare-review')
    expect(primaryOrbAction(snapshot, now)).toMatchObject({ kind: 'compact-guide', label: '查看压缩指引' })
    expect(primaryOrbAction({ ...snapshot, usedTokens: 20_000 }, now).kind).toBe('view-status')
    expect(primaryOrbAction({ ...snapshot, source: 'codex-hook' }, now).kind).toBe('view-status')
    expect(primaryOrbAction(snapshot, now + 6 * 60_000).kind).toBe('view-status')
  })
  it('never treats a file failure, a recorded suspicion, or missing evidence as handoff authorization', () => {
    expect(primaryOrbAction(demoSessions('failed', now)[0], now).kind).toBe('view-evidence')
    expect(primaryOrbAction(demoSessions('unknown', now)[0], now).kind).toBe('prepare-review')
    const snapshot = demoSessions('failed', now)[0]
    const report = { ...snapshot.report!, probes: snapshot.report!.probes.map(probe => ({ ...probe, result: 'pass' as const })) }
    expect(primaryOrbAction({ ...snapshot, report }, now).kind).toBe('view-evidence')
  })
  it('cancels a held intent after target, evidence, or applicable action changes', () => {
    const snapshot = demoSessions('passed', now)[0]
    const intent = captureOrbIntent(primaryOrbAction(snapshot, now))
    expect(isCurrentOrbIntent(intent, primaryOrbAction(snapshot, now))).toBe(true)
    for (const next of [
      { ...snapshot, id: 'another-task' },
      { ...snapshot, report: { ...snapshot.report!, report_id: 'f'.repeat(64) } },
      { ...snapshot, usedTokens: 10_000 },
      { ...snapshot, source: 'codex-hook' as const },
    ]) expect(isCurrentOrbIntent(intent, primaryOrbAction(next, now))).toBe(false)
  })
})
