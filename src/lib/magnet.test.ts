import { describe, expect, it } from 'vitest'
import { DEFAULT_MAGNET, dockPreview, parseMagnetPreferences } from './magnet'

describe('preview geometry and saved preferences', () => {
  it('does not restore arbitrary thresholds or unsupported window modes', () => {
    expect(parseMagnetPreferences('{bad')).toEqual(DEFAULT_MAGNET)
    expect(parseMagnetPreferences('{"screenEdges":false,"windowMode":"cli"}')).toEqual(DEFAULT_MAGNET)
    expect(parseMagnetPreferences('{"enabled":false,"screenEdges":false,"windowMode":"all","snapDistance":9999}')).toEqual({ ...DEFAULT_MAGNET, windowMode: 'all' })
  })
  it('docks a distant release to the nearest screen edge, including window mode off', () => {
    const area = { x: 0, y: 0, width: 1200, height: 800 }
    const ball = { x: 650, y: 350, width: 80, height: 80 }
    for (const windowMode of ['codex', 'off', 'all'] as const) {
      const result = dockPreview(ball, area, [], { ...DEFAULT_MAGNET, windowMode }, { x: 690, y: 390 })
      expect(result).toMatchObject({ kind: 'screen', side: 'top', rect: { x: 650, y: 0 } })
    }
  })
  it('uses the containing window only and ranks edges by release cursor', () => {
    const area = { x: 0, y: 0, width: 1200, height: 800 }
    const window = { x: 200, y: 150, width: 600, height: 400 }
    const ball = { x: 250, y: 310, width: 80, height: 80 }
    expect(dockPreview(ball, area, [window], DEFAULT_MAGNET, { x: 290, y: 350 })).toMatchObject({ kind: 'window', side: 'left', rect: { x: 120 } })
    expect(dockPreview(ball, area, [window], DEFAULT_MAGNET, { x: 190, y: 350 })).toMatchObject({ kind: 'screen', side: 'left', rect: { x: 0 } })
    expect(dockPreview(ball, area, [window], { ...DEFAULT_MAGNET, windowMode: 'off' }, { x: 290, y: 350 })).toMatchObject({ kind: 'screen', side: 'left', rect: { x: 0 } })
  })
  it('uses an inner window edge if the exterior would leave the work area', () => {
    const area = { x: -1200, y: 0, width: 1200, height: 800 }
    const window = { x: -1190, y: 100, width: 600, height: 500 }
    expect(dockPreview({ x: -1140, y: 250, width: 80, height: 80 }, area, [window], DEFAULT_MAGNET, { x: -1130, y: 290 }))
      .toMatchObject({ kind: 'window', side: 'left', rect: { x: -1190 } })
  })
})
