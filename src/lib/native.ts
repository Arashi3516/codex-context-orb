import type { ContextSnapshot, SemanticAssessment } from './context'

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

export function mergeLocalSessions(events: HookSnapshot[], reviews: SemanticAssessment[]): ContextSnapshot[] {
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
  return [...merged.values()].sort((a, b) => b.observedAt - a.observedAt)
}

export async function readLocalSessions(pinnedId: string | null): Promise<ContextSnapshot[]> {
  if (!isNative) return []
  const { invoke } = await import('@tauri-apps/api/core')
  const [events, reviews, pinnedEvents, pinnedReviews] = await Promise.all([
    invoke<HookSnapshot[]>('read_hook_events', { sessionId: null }),
    invoke<SemanticAssessment[]>('read_semantic_assessments', { sessionId: null }),
    pinnedId ? invoke<HookSnapshot[]>('read_hook_events', { sessionId: pinnedId }) : [],
    pinnedId ? invoke<SemanticAssessment[]>('read_semantic_assessments', { sessionId: pinnedId }) : [],
  ])
  return mergeLocalSessions([...events, ...pinnedEvents], [...reviews, ...pinnedReviews])
}

export async function sizeOrbWindow(expanded: boolean) {
  if (!isNative) return
  const { getCurrentWindow, currentMonitor } = await import('@tauri-apps/api/window')
  const { LogicalSize, PhysicalPosition } = await import('@tauri-apps/api/dpi')
  const nativeWindow = getCurrentWindow()
  const [position, size, scale, monitor] = await Promise.all([
    nativeWindow.outerPosition(), nativeWindow.outerSize(),
    nativeWindow.scaleFactor(), currentMonitor(),
  ])
  const width = expanded ? 382 : 92
  const height = expanded ? 690 : 92
  const nextWidth = Math.round(width * scale)
  const nextHeight = Math.round(height * scale)
  const area = monitor?.workArea
  const x = Math.max(area?.position.x ?? 0, position.x + size.width - nextWidth)
  const y = Math.max(area?.position.y ?? 0, position.y + size.height - nextHeight)
  await nativeWindow.setSize(new LogicalSize(width, height))
  await nativeWindow.setPosition(new PhysicalPosition(x, y))
}

export async function dragNativeWindow() {
  if (!isNative) return
  const { getCurrentWindow } = await import('@tauri-apps/api/window')
  await getCurrentWindow().startDragging()
}
