export type WindowMagnetMode = 'codex' | 'off' | 'all'
export interface MagnetPreferences {
  windowMode: WindowMagnetMode
}
export interface Rect { x: number; y: number; width: number; height: number }
type Capability = 'available' | 'unavailable' | 'unsupported'
export interface MagnetState {
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
  const target = prefs.windowMode === 'off' ? undefined : windows.find(contains)
  const edge = (r: Rect) => ([
    { side: 'left', distance: Math.abs(cursor.x - r.x) }, { side: 'right', distance: Math.abs(cursor.x - r.x - r.width) },
    { side: 'top', distance: Math.abs(cursor.y - r.y) }, { side: 'bottom', distance: Math.abs(cursor.y - r.y - r.height) },
  ] as const).slice().sort((a, b) => a.distance - b.distance)[0].side
  const clamp = (n: number, min: number, max: number) => Math.max(min, Math.min(max, n))
  const fit = (r: Rect, side: string, outside: boolean): Rect => {
    let x = clamp(rect.x, area.x, area.x + area.width - rect.width)
    let y = clamp(rect.y, area.y, area.y + area.height - rect.height)
    if (side === 'left') x = r.x - (outside ? rect.width : 0)
    if (side === 'right') x = r.x + r.width - (outside ? 0 : rect.width)
    if (side === 'top') y = r.y - (outside ? rect.height : 0)
    if (side === 'bottom') y = r.y + r.height - (outside ? 0 : rect.height)
    return { ...rect, x, y }
  }
  const fits = (r: Rect) => r.x >= area.x && r.y >= area.y && r.x + r.width <= area.x + area.width && r.y + r.height <= area.y + area.height
  if (target) {
    const side = edge(target)
    for (const outside of [true, false]) {
      const docked = fit(target, side, outside)
      if (fits(docked)) return { rect: docked, kind: 'window' as const, side }
    }
  }
  const side = edge(area)
  return { rect: fit(area, side, false), kind: 'screen' as const, side }
}
