import { afterEach, describe, expect, it, vi } from 'vitest'
import { evaluateContext } from './context'
import { demoReport, legacyDemoAssessment, DEMO_PRIMARY_ID } from './demo'
import { mergeLocalSessions } from './native'
import type { HookSnapshot } from './native'

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke }))
const now = 2_000_000
function event(observedAt = now, lastEvent = 'Stop'): HookSnapshot {
  return { schema_version: 1, source: 'codex-hook', session_id: DEMO_PRIMARY_ID,
    turn_id: 'demo-turn-30', observed_at_ms: observedAt, last_event_name: lastEvent,
    model: null, context_used_tokens: null, context_window_tokens: null, binding: 'unbound' }
}
afterEach(() => { vi.unstubAllGlobals(); vi.resetAllMocks(); vi.resetModules() })

describe('as-of reports and lifecycle observations', () => {
  it('merges the newest hook separately from the evidence snapshot', () => {
    for (const events of [[event(now - 1), event(now + 1, 'PostCompact')], [event(now + 1, 'PostCompact'), event(now - 1)]]) {
      const [item] = mergeLocalSessions(events, [], [demoReport('passed', now)])
      expect(item.observedAt).toBe(now + 1)
      expect(item.lastEvent).toBe('PostCompact')
      expect(evaluateContext(item, now + 1).label).toBe('所列检查通过')
      expect(evaluateContext(item, now + 1).notices.join('')).toContain('新的会话活动')
    }
  })
  it('never infers an active turn from a self-declared report', () => {
    const old = demoReport('failed', now), newer = { ...demoReport('passed', now + 1), turn_id: 'report-turn' }
    for (const reports of [[old, newer], [newer, old]]) {
      const [item] = mergeLocalSessions([], [], reports)
      expect(item.turnId).toBeNull()
      expect(item.report).toBe(newer)
      expect(item.lastEvent).toBeUndefined()
    }
  })
  it('keeps a known hook turn and the old legacy report without replacing v2', () => {
    const [item] = mergeLocalSessions([{ ...event(now + 2), turn_id: 'newer-turn' }], [legacyDemoAssessment(now + 1)], [demoReport('passed', now)])
    expect(item.turnId).toBe('newer-turn')
    expect(item.assessment?.schema_version).toBe(1)
    expect(item.report?.schema_version).toBe(2)
    expect(item.source).toBe('codex-evidence-review')
  })
})

describe('exact native selection', () => {
  it('reads all pinned sources exactly even when inventories omit the session', async () => {
    vi.stubGlobal('window', { __TAURI_INTERNALS__: {} })
    invoke.mockImplementation((command: string, args: { sessionId: string | null }) => {
      if (!args.sessionId) return Promise.resolve([])
      return Promise.resolve(command === 'read_hook_events' ? [event(now + 1)] : command === 'read_evidence_reports' ? [demoReport('passed', now)] : [])
    })
    const { readLocalSessions } = await import('./native')
    const [item] = await readLocalSessions(DEMO_PRIMARY_ID)
    expect(invoke).toHaveBeenCalledWith('read_evidence_reports', { sessionId: DEMO_PRIMARY_ID })
    expect(invoke).toHaveBeenCalledWith('read_hook_events', { sessionId: DEMO_PRIMARY_ID })
    expect(item.id).toBe(DEMO_PRIMARY_ID)
    expect(item.report?.session_id).toBe(DEMO_PRIMARY_ID)
  })
  it('propagates unreadable pinned evidence instead of hiding it behind legacy data', async () => {
    vi.stubGlobal('window', { __TAURI_INTERNALS__: {} })
    invoke.mockImplementation((command: string, args: { sessionId: string | null }) => {
      if (command === 'read_evidence_reports' && args.sessionId) return Promise.reject(new Error('unreadable report'))
      return Promise.resolve(command === 'read_semantic_assessments' ? [legacyDemoAssessment(now)] : [])
    })
    const { readLocalSessions } = await import('./native')
    await expect(readLocalSessions(DEMO_PRIMARY_ID)).rejects.toThrow('unreadable report')
  })
  it('does not add an implicit selected session', async () => {
    vi.stubGlobal('window', { __TAURI_INTERNALS__: {} })
    invoke.mockResolvedValue([])
    const { readLocalSessions } = await import('./native')
    expect(await readLocalSessions(null)).toEqual([])
    expect(invoke).toHaveBeenCalledTimes(3)
    expect(invoke).toHaveBeenCalledWith('read_evidence_reports', { sessionId: null })
  })
  it('requests history for one explicit session', async () => {
    vi.stubGlobal('window', { __TAURI_INTERNALS__: {} })
    invoke.mockResolvedValue([demoReport('passed', now)])
    const { readEvidenceHistory } = await import('./native')
    expect(await readEvidenceHistory(DEMO_PRIMARY_ID)).toHaveLength(1)
    expect(invoke).toHaveBeenCalledWith('read_evidence_history', { sessionId: DEMO_PRIMARY_ID })
  })
})
