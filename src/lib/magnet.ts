export type WindowMagnetMode = 'codex' | 'off' | 'all'
export interface MagnetPreferences {
  windowMode: WindowMagnetMode
}
export interface Rect { x: number; y: number; width: number; height: number }
type Capability = 'available' | 'unavailable' | 'unsupported'
export interface MagnetState {
  revision: number
  preferences: MagnetPreferences
  capabilities: { drag: Capability; screenEdges: Capability; windowEdges: Capability; coordinateSpace: 'logical_points' | 'physical_pixels' | 'unsupported'; codexGui: 'bundle_id' | 'unavailable'; reason: string | null }
  dragging: boolean
  lastDragMoved: boolean
  snappedX: 'screen' | 'window' | null
  snappedY: 'screen' | 'window' | null
  snapSideX: 'left' | 'right' | null
  snapSideY: 'top' | 'bottom' | null
  layoutAnchorLeft: boolean
  layoutAnchorTop: boolean
  windowTargets: number
  geometry: Rect | null
  monitor: { workArea: Rect; unitsPerLogicalPixel: number } | null
  lastError: string | null
}
export function latestMagnetState(current: MagnetState | null, incoming: MagnetState): MagnetState {
  return current && (current.revision ?? 0) > (incoming.revision ?? 0) ? current : incoming
}
export const DEFAULT_MAGNET: MagnetPreferences = { windowMode: 'codex' }
export const WINDOW_MODE_LABELS: Record<WindowMagnetMode, string> = { codex: '仅限 Codex', off: '关闭', all: '所有窗口' }

export function parseMagnetPreferences(raw: string): MagnetPreferences {
  try {
    const value = JSON.parse(raw)
    if (['codex', 'off', 'all'].includes(value?.windowMode)) {
      return { ...DEFAULT_MAGNET, windowMode: value.windowMode }
    }
  } catch { /* an optional preference is never authoritative */ }
  return { ...DEFAULT_MAGNET }
}

/** Browser demo of mandatory release docking. Desktop geometry remains native. */
export function dockPreview(rect: Rect, area: Rect, windows: Rect[], prefs: MagnetPreferences, cursor: { x: number; y: number }) {
  const contains = (r: Rect) => cursor.x >= r.x && cursor.x <= r.x + r.width && cursor.y >= r.y && cursor.y <= r.y + r.height
  const overlaps = (r: Rect) => rect.x <= r.x + r.width && rect.x + rect.width >= r.x
    && rect.y <= r.y + r.height && rect.y + rect.height >= r.y
  const target = prefs.windowMode === 'off' ? undefined : windows.find(contains) ?? windows.find(overlaps)
  const edge = (r: Rect) => ([
    { side: 'left', distance: Math.abs(cursor.x - r.x) }, { side: 'right', distance: Math.abs(cursor.x - r.x - r.width) },
    { side: 'top', distance: Math.abs(cursor.y - r.y) }, { side: 'bottom', distance: Math.abs(cursor.y - r.y - r.height) },
  ] as const).slice().sort((a, b) => a.distance - b.distance)[0].side
  const clamp = (n: number, min: number, max: number) => Math.max(min, Math.min(max, n))
  const fit = (r: Rect, side: string, bounds = area): Rect => {
    let x = clamp(rect.x, bounds.x, bounds.x + bounds.width - rect.width)
    let y = clamp(rect.y, bounds.y, bounds.y + bounds.height - rect.height)
    if (side === 'left') x = r.x
    if (side === 'right') x = r.x + r.width - rect.width
    if (side === 'top') y = r.y
    if (side === 'bottom') y = r.y + r.height - rect.height
    return { ...rect, x: clamp(x, bounds.x, bounds.x + bounds.width - rect.width),
      y: clamp(y, bounds.y, bounds.y + bounds.height - rect.height) }
  }
  const fits = (r: Rect, bounds: Rect) => r.x >= bounds.x && r.y >= bounds.y && r.x + r.width <= bounds.x + bounds.width && r.y + r.height <= bounds.y + bounds.height
  if (target) {
    const side = edge(target)
    const x = Math.max(area.x, target.x), y = Math.max(area.y, target.y)
    const visible = { x, y, width: Math.min(area.x + area.width, target.x + target.width) - x,
      height: Math.min(area.y + area.height, target.y + target.height) - y }
    if (visible.width >= rect.width && visible.height >= rect.height) {
      const docked = fit(target, side, visible)
      if (fits(docked, visible)) return { rect: docked, kind: 'window' as const, side }
    }
  }
  const side = edge(area)
  return { rect: fit(area, side), kind: 'screen' as const, side }
}
