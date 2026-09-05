import type { ContextSnapshot } from './context'

export const isNative = '__TAURI_INTERNALS__' in window

interface HookSnapshot {
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

export async function readHookSessions(): Promise<ContextSnapshot[]> {
  if (!isNative) return []
  const { invoke } = await import('@tauri-apps/api/core')
  const data = await invoke<HookSnapshot[]>('read_hook_events')
  return data.map(event => ({
    id: event.session_id,
    title: `会话 · ${event.session_id.length > 18 ? `${event.session_id.slice(0, 8)}…${event.session_id.slice(-6)}` : event.session_id}`,
    source: 'codex-hook',
    model: event.model,
    observedAt: event.observed_at_ms,
    usedTokens: null,
    windowTokens: null,
    compactions: null,
    lastEvent: event.last_event_name,
  }))
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
  const height = expanded ? 628 : 92
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
