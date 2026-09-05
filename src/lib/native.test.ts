import { afterEach, describe, expect, it, vi } from 'vitest'
import { evaluateContext } from './context'
import { demoAssessment, DEMO_PRIMARY_ID } from './demo'
import { mergeLocalSessions } from './native'
import type { HookSnapshot } from './native'

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke }))

const now = 2_000_000
function event(observedAt = now, lastEvent = 'Stop'): HookSnapshot {
  return {
    schema_version: 1, source: 'codex-hook', session_id: DEMO_PRIMARY_ID,
    turn_id: 'demo-turn-30', observed_at_ms: observedAt, last_event_name: lastEvent,
    model: null, context_used_tokens: null, context_window_tokens: null, binding: 'unbound',
  }
}

afterEach(() => {
  vi.unstubAllGlobals()
  vi.resetAllMocks()
  vi.resetModules()
})

describe('local assessment and lifecycle merge', () => {
  it('keeps the newest hook when inventory and exact reads overlap', () => {
    const older = event(now - 1), newer = event(now + 1, 'PostCompact')
    for (const events of [[older, newer], [newer, older]]) {
      const [snapshot] = mergeLocalSessions(events, [demoAssessment('handoff', now)])
      expect(snapshot.observedAt).toBe(now + 1)
      expect(snapshot.lastEvent).toBe('PostCompact')
      expect(evaluateContext(snapshot, now + 1).level).toBe('unknown')
    }
  })
  it('keeps a later Stop invalid even after it replaces the compact event', () => {
    const report = demoAssessment('handoff', now)
    const [compacted] = mergeLocalSessions([event(now + 1, 'PostCompact')], [report])
    const [stopped] = mergeLocalSessions([event(now + 2, 'Stop')], [report])
    expect(evaluateContext(compacted, now + 2).level).toBe('unknown')
    expect(evaluateContext(stopped, now + 2).level).toBe('unknown')
  })
  it('does not invent an observed active turn from report-only data', () => {
    const oldReview = demoAssessment('handoff', now)
    const newReview = { ...oldReview, turn_id: 'new-reviewed-turn', reviewed_at_ms: now + 1 }
    for (const reviews of [[oldReview, newReview], [newReview, oldReview]]) {
      const [snapshot] = mergeLocalSessions([], reviews)
      expect(snapshot.turnId).toBeNull()
      expect(snapshot.lastEvent).toBeUndefined()
      expect(snapshot.observedAt).toBe(now + 1)
      expect(snapshot.assessment).toBe(newReview)
    }
  })
  it('keeps a known newer hook turn when the report turn is null', () => {
    const report = { ...demoAssessment('healthy', now), turn_id: null }
    const [snapshot] = mergeLocalSessions([{ ...event(now + 1), turn_id: 'new-turn' }], [report])
    expect(snapshot.turnId).toBe('new-turn')
    expect(evaluateContext(snapshot, now + 1).level).toBe('unknown')
  })
})

describe('pinned native reads', () => {
  it('reads both pinned sources exactly when inventories omit the session', async () => {
    vi.stubGlobal('window', { __TAURI_INTERNALS__: {} })
    invoke.mockImplementation((command: string, args: { sessionId: string | null }) => {
      if (args.sessionId === null) return Promise.resolve([])
      return Promise.resolve(command === 'read_hook_events' ? [event(now + 1)] : [demoAssessment('handoff', now)])
    })
    const { readLocalSessions } = await import('./native')
    const [snapshot] = await readLocalSessions(DEMO_PRIMARY_ID)
    expect(invoke).toHaveBeenCalledWith('read_hook_events', { sessionId: DEMO_PRIMARY_ID })
    expect(invoke).toHaveBeenCalledWith('read_semantic_assessments', { sessionId: DEMO_PRIMARY_ID })
    expect(snapshot.id).toBe(DEMO_PRIMARY_ID)
    expect(evaluateContext(snapshot, now + 1).level).toBe('unknown')
  })
  it('does not hide an unreadable pinned hook behind a valid report', async () => {
    vi.stubGlobal('window', { __TAURI_INTERNALS__: {} })
    invoke.mockImplementation((command: string, args: { sessionId: string | null }) => {
      if (command === 'read_hook_events' && args.sessionId) return Promise.reject(new Error('synthetic unreadable hook'))
      return Promise.resolve(command === 'read_semantic_assessments' ? [demoAssessment('handoff', now)] : [])
    })
    const { readLocalSessions } = await import('./native')
    await expect(readLocalSessions(DEMO_PRIMARY_ID)).rejects.toThrow('synthetic unreadable hook')
  })
  it('does not choose a session implicitly when nothing is pinned', async () => {
    vi.stubGlobal('window', { __TAURI_INTERNALS__: {} })
    invoke.mockResolvedValue([])
    const { readLocalSessions } = await import('./native')
    expect(await readLocalSessions(null)).toEqual([])
    expect(invoke).toHaveBeenCalledTimes(2)
    expect(invoke).toHaveBeenCalledWith('read_hook_events', { sessionId: null })
  })
})
