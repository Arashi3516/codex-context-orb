import type { ContextSnapshot, SemanticAssessment } from './context'
import type { EvidenceReport } from './evidence'
import type { MagnetPreferences, MagnetState } from './magnet'

export const isNative = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

export interface HookSnapshot {
  schema_version: number
  source: 'codex-hook'
  session_id: string
  turn_id: string | null
  observed_at_ms: number
  last_event_name: string
  model: string | null
  context_used_tokens: null
  context_window_tokens: null
  binding: 'unbound'
}

function sessionTitle(id: string) {
  return `会话 · ${id.length > 18 ? `${id.slice(0, 8)}…${id.slice(-6)}` : id}`
}

export function mergeLocalSessions(events: HookSnapshot[], reviews: SemanticAssessment[], reports: EvidenceReport[] = []): ContextSnapshot[] {
  const merged = new Map<string, ContextSnapshot>()
  for (const event of events) {
    const existing = merged.get(event.session_id)
    // Inventory and exact reads may finish in either order. Keep the newer observation.
    if (existing && existing.observedAt >= event.observed_at_ms) continue
    merged.set(event.session_id, {
      id: event.session_id, title: sessionTitle(event.session_id), source: 'codex-hook',
      model: event.model, observedAt: event.observed_at_ms, turnId: event.turn_id,
      usedTokens: null, windowTokens: null, compactions: null,
      lastEvent: event.last_event_name, assessment: null,
    })
  }
  for (const review of reviews) {
    const existing = merged.get(review.session_id)
    if (existing?.assessment && existing.assessment.reviewed_at_ms > review.reviewed_at_ms) continue
    const hasHook = existing?.lastEvent !== undefined
    merged.set(review.session_id, {
      id: review.session_id, title: sessionTitle(review.session_id), source: 'codex-skill-review',
      model: existing?.model ?? null, observedAt: hasHook ? existing.observedAt : review.reviewed_at_ms,
      // A report's claimed turn is not an independent observation of the active turn.
      turnId: hasHook ? existing.turnId : null,
      usedTokens: null, windowTokens: null, compactions: review.compactions_observed,
      lastEvent: existing?.lastEvent, assessment: review,
    })
  }
  for (const report of reports) {
    const existing = merged.get(report.session_id)
    if (existing?.report && existing.report.reviewed_at_ms >= report.reviewed_at_ms) continue
    const hasHook = existing?.lastEvent !== undefined
    merged.set(report.session_id, {
      id: report.session_id, title: sessionTitle(report.session_id), source: 'codex-evidence-review',
      model: existing?.model ?? null, observedAt: hasHook ? existing.observedAt : report.reviewed_at_ms,
      turnId: hasHook ? existing.turnId : null, lastEvent: existing?.lastEvent,
      usedTokens: null, windowTokens: null, compactions: null,
      assessment: existing?.assessment, report,
    })
  }
  return [...merged.values()].sort((a, b) => b.observedAt - a.observedAt)
}

export async function readLocalSessions(pinnedId: string | null): Promise<ContextSnapshot[]> {
  if (!isNative) return []
  const { invoke } = await import('@tauri-apps/api/core')
  const [events, reviews, reports, pinnedEvents, pinnedReviews, pinnedReports] = await Promise.all([
    invoke<HookSnapshot[]>('read_hook_events', { sessionId: null }),
    invoke<SemanticAssessment[]>('read_semantic_assessments', { sessionId: null }),
    invoke<EvidenceReport[]>('read_evidence_reports', { sessionId: null }),
    pinnedId ? invoke<HookSnapshot[]>('read_hook_events', { sessionId: pinnedId }) : [],
    pinnedId ? invoke<SemanticAssessment[]>('read_semantic_assessments', { sessionId: pinnedId }) : [],
    pinnedId ? invoke<EvidenceReport[]>('read_evidence_reports', { sessionId: pinnedId }) : [],
  ])
  return mergeLocalSessions([...events, ...pinnedEvents], [...reviews, ...pinnedReviews], [...reports, ...pinnedReports])
}

export async function readEvidenceHistory(sessionId: string): Promise<EvidenceReport[]> {
  if (!isNative) return []
  const { invoke } = await import('@tauri-apps/api/core')
  return invoke<EvidenceReport[]>('read_evidence_history', { sessionId })
}

export async function sizeOrbWindow(expanded: boolean) {
  if (!isNative) return
  return magnetCommand('resize_orb_window', { width: expanded ? 382 : 92, height: expanded ? 690 : 92 })
}

async function magnetCommand(command: string, args?: Record<string, unknown>): Promise<MagnetState> {
  const { invoke } = await import('@tauri-apps/api/core')
  return invoke<MagnetState>(command, args)
}

export const getNativeMagnetState = () => magnetCommand('get_magnet_state')
export const setNativeMagnetPreferences = (preferences: MagnetPreferences) => magnetCommand('set_magnet_preferences', { preferences })
export const beginNativeMagneticDrag = (anchorX: number, anchorY: number) => magnetCommand('begin_magnetic_drag', { anchorX, anchorY })
export const endNativeMagneticDrag = (release?: { anchorX: number; anchorY: number; moved: boolean }) => magnetCommand('end_magnetic_drag', { ...release, reducedMotion: matchMedia('(prefers-reduced-motion: reduce)').matches })
