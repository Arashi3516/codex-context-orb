import { describe, expect, it } from 'vitest'
import { DEFAULT_MAGNET, dockPreview, latestMagnetState, parseMagnetPreferences, type MagnetState } from './magnet'

describe('preview geometry and saved preferences', () => {
  it('rejects a late poll that would restore an old native layout corner', () => {
    const before = { revision: 10, layoutAnchorLeft: false, layoutAnchorTop: false } as MagnetState
    const after = { revision: 12, layoutAnchorLeft: true, layoutAnchorTop: true } as MagnetState
    expect(latestMagnetState(after, before)).toBe(after)
    expect(latestMagnetState(before, after)).toBe(after)
    expect(latestMagnetState(null, after)).toBe(after)
  })
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
  it('accepts window interiors and the ball footprint, ranking edges by release cursor', () => {
    const area = { x: 0, y: 0, width: 1200, height: 800 }
    const window = { x: 200, y: 150, width: 600, height: 400 }
    const ball = { x: 250, y: 310, width: 80, height: 80 }
    expect(dockPreview(ball, area, [window], DEFAULT_MAGNET, { x: 290, y: 350 })).toMatchObject({ kind: 'window', side: 'left', rect: { x: 120 } })
    expect(dockPreview({ ...ball, x: 150 }, area, [window], DEFAULT_MAGNET, { x: 190, y: 350 })).toMatchObject({ kind: 'window', side: 'left', rect: { x: 120 } })
    expect(dockPreview({ ...ball, x: 110 }, area, [window], DEFAULT_MAGNET, { x: 150, y: 350 })).toMatchObject({ kind: 'screen', side: 'left', rect: { x: 0 } })
    expect(dockPreview(ball, area, [window], { ...DEFAULT_MAGNET, windowMode: 'off' }, { x: 290, y: 350 })).toMatchObject({ kind: 'screen', side: 'left', rect: { x: 0 } })
  })
  it('captures all four window borders when the grabbed point falls just outside', () => {
    const area = { x: 0, y: 0, width: 1200, height: 800 }
    const window = { x: 200, y: 150, width: 600, height: 400 }
    for (const [x, y, side] of [[199, 350, 'left'], [801, 350, 'right'], [500, 149, 'top'], [500, 551, 'bottom']] as const) {
      expect(dockPreview({ x: x - 40, y: y - 40, width: 80, height: 80 }, area, [window], DEFAULT_MAGNET, { x, y }))
        .toMatchObject({ kind: 'window', side })
    }
  })
  it('uses an inner window edge if the exterior would leave the work area', () => {
    const area = { x: -1200, y: 0, width: 1200, height: 800 }
    const window = { x: -1190, y: 100, width: 600, height: 500 }
    expect(dockPreview({ x: -1140, y: 250, width: 80, height: 80 }, area, [window], DEFAULT_MAGNET, { x: -1130, y: 290 }))
      .toMatchObject({ kind: 'window', side: 'left', rect: { x: -1190 } })
  })
})
