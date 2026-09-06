import { describe, expect, it } from 'vitest'
import { LIQUID_TRANSITION_MS, liquidPixelSize, liquidWeights, retargetLiquidTransition,
  sampleLiquidTransition, settledLiquidTransition } from './orb-liquid'

describe('liquid state transitions', () => {
  it('melts between states without jumping or overshooting', () => {
    const start = settledLiquidTransition('aligned', 0)
    const transition = retargetLiquidTransition(start, 'deviation', 100)
    expect(sampleLiquidTransition(transition, 100)).toEqual(liquidWeights('aligned'))
    expect(sampleLiquidTransition(transition, 100 + LIQUID_TRANSITION_MS / 2)).toEqual([0, .5, 0, .5])
    expect(sampleLiquidTransition(transition, 100 + LIQUID_TRANSITION_MS)).toEqual(liquidWeights('deviation'))
    for (const time of [-100, 100, 210, 510, 810, 10_000]) {
      const weights = sampleLiquidTransition(transition, time)
      expect(weights.reduce((sum, value) => sum + value, 0)).toBeCloseTo(1)
      expect(weights.every(value => value >= 0 && value <= 1)).toBe(true)
    }
  })
  it('retargets an unfinished transition from its current appearance', () => {
    const first = retargetLiquidTransition(settledLiquidTransition('unknown', 0), 'review', 10)
    const interruptedAt = 310
    const visible = sampleLiquidTransition(first, interruptedAt)
    const second = retargetLiquidTransition(first, 'deviation', interruptedAt)
    expect(sampleLiquidTransition(second, interruptedAt)).toEqual(visible)
    expect(sampleLiquidTransition(second, interruptedAt + LIQUID_TRANSITION_MS)).toEqual(liquidWeights('deviation'))
  })
  it('settles a static state without resuming an old transition later', () => {
    const transition = settledLiquidTransition('review', 400)
    expect(sampleLiquidTransition(transition, 400)).toEqual(liquidWeights('review'))
    expect(sampleLiquidTransition(transition, 20_000)).toEqual(liquidWeights('review'))
  })
  it('bounds the real canvas at 70 logical pixels and at most double density', () => {
    expect([0, 1, 1.5, 2, 3, Infinity, NaN].map(liquidPixelSize)).toEqual([70, 70, 105, 140, 140, 70, 70])
  })
})
