import type { ContextSnapshot, HealthLevel, SemanticAssessment, SemanticSignal } from './context'

export const DEMO_PRIMARY_ID = 'demo-current'
export const DEMO_BACKGROUND_ID = 'demo-background'

export function demoAssessment(level: HealthLevel, now = Date.now()): SemanticAssessment {
  const signals: SemanticSignal[] = [
    {
      id: 'constraint-01', kind: 'constraint_loss', status: 'open', after_compaction: true,
      affects_next_step: true, recurrence: 'after_correction', confidence: 'high',
      summary: '“保留现有保存入口”已明确两次，后续修改仍再次移除了它。',
      evidence: [
        { ref: '演示 · 第 18 轮用户', note: '当前约束：保留现有保存入口，只统一设置项层级。' },
        { ref: '演示 · 第 24 轮用户', note: '已纠正：保存入口不能删除，后续修改继续沿用此约束。' },
        { ref: '演示 · 第 28 轮执行', note: '压缩后的修改再次移除了保存入口，下一步验收会偏离要求。' },
      ],
    },
    {
      id: 'stale-01', kind: 'stale_fact', status: 'open', after_compaction: true,
      affects_next_step: true, recurrence: 'once', confidence: 'high',
      summary: '已作废的自动保存方案重新成为实现前提，与当前交互要求冲突。',
      evidence: [
        { ref: '演示 · 第 20 轮决策', note: '自动保存方案已否决；这次仍采用用户主动保存。' },
        { ref: '演示 · 第 29 轮执行', note: '后续实现按自动保存前提改动反馈，和已确认的决定不一致。' },
      ],
    },
  ]
  return {
    schema_version: 1, source: 'codex-skill-review', session_id: DEMO_PRIMARY_ID,
    turn_id: 'demo-turn-30', reviewed_at_ms: now, compactions_observed: level === 'healthy' ? 8 : 4,
    coverage: level === 'unknown' ? 'partial' : 'sufficient',
    current_goal: '统一设置页交互，保留用户主动保存入口。',
    next_step: '核对仍有效的保存约束，再检查键盘导航与反馈。',
    review_note: level === 'healthy'
      ? '虽然已有多次压缩，当前目标、有效约束和最近执行仍一致。未把长会话当作混乱。'
      : '已排除正常需求调整、已解决的问题和单纯工具报错；这里只保留影响当前下一步的疑点。',
    signals: level === 'handoff' ? signals : level === 'watch' ? [{ ...signals[0], recurrence: 'once', confidence: 'medium', summary: '最近修改似乎遗漏了保存约束，需要与当前要求核对。', evidence: [signals[0].evidence[0], signals[0].evidence[2]] }] : [],
  }
}

export function demoSessions(level: HealthLevel, now = Date.now()): ContextSnapshot[] {
  return [
    {
      id: DEMO_PRIMARY_ID, title: '设置页交互优化', model: '示例模型', source: 'demo',
      observedAt: now, turnId: 'demo-turn-30', usedTokens: level === 'healthy' ? 196_000 : 48_000,
      windowTokens: 200_000, compactions: level === 'healthy' ? 8 : 4,
      assessment: level === 'unknown' ? null : demoAssessment(level, now),
    },
    {
      id: DEMO_BACKGROUND_ID, title: 'API 重试边界检查', model: '示例模型', source: 'demo',
      observedAt: now, turnId: 'demo-turn-b', usedTokens: 196_000, windowTokens: 200_000, compactions: 8,
      assessment: { ...demoAssessment('healthy', now), session_id: DEMO_BACKGROUND_ID, turn_id: 'demo-turn-b', current_goal: '检查 API 重试边界。', next_step: '核对已确认的重试次数与超时约束。' },
    },
  ]
}
