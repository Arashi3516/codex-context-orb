import type { ContextSnapshot, HealthLevel } from './context'

export const DEMO_PRIMARY_ID = 'demo-current'
export const DEMO_BACKGROUND_ID = 'demo-background'

export function demoSessions(level: HealthLevel, now = Date.now()): ContextSnapshot[] {
  const used = { healthy: 84_000, watch: 164_000, handoff: 188_000, unknown: null }[level]
  return [
    {
      id: DEMO_PRIMARY_ID, title: '设置页交互优化', model: '示例模型', source: 'demo',
      observedAt: now, usedTokens: used, windowTokens: level === 'unknown' ? null : 200_000,
      compactions: level === 'watch' || level === 'handoff' ? 2 : 0,
    },
    {
      id: DEMO_BACKGROUND_ID, title: 'API 重试边界检查', model: '示例模型', source: 'demo',
      observedAt: now, usedTokens: 196_000, windowTokens: 200_000, compactions: 3,
    },
  ]
}
