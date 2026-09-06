import type { ContextSnapshot, SemanticAssessment } from './context'
import type { EvidenceReport } from './evidence'

export const DEMO_PRIMARY_ID = 'demo-current'
export const DEMO_BACKGROUND_ID = 'demo-background'
export type DemoScenario = 'passed' | 'failed' | 'superseded' | 'unknown'

/** Every byte here is synthetic. No fixture is a receipt from a real user session. */
export function demoReport(scenario: DemoScenario, now = Date.now()): EvidenceReport {
  const passed = scenario === 'passed' || scenario === 'superseded'
  const missing = scenario === 'unknown'
  return {
    schema_version: 2, source: 'codex-evidence-review', report_id: (passed ? 'a' : missing ? 'c' : 'b').repeat(64),
    session_id: DEMO_PRIMARY_ID, turn_id: 'demo-turn-30', reviewed_at_ms: now,
    scope: { mode: 'as_of', origin: 'main', action_id: 'settings-review', goal_id: 'goal-settings',
      next_step: '核对保存方式，再检查键盘导航与错误反馈。', coverage: 'declared', unknowns: missing ? ['键盘导航检查需要实际交互，尚未核验。'] : [] },
    sources: [
      { id: 'request', kind: 'statement', ref: '演示 · 第 24 轮明确要求', note: '保留主动保存，不采用自动保存。由本次评估整理。', status: 'attested', sha256: null },
      { id: 'settings', kind: 'artifact', ref: 'src/settings.ts', note: '演示设置文件；版本与检查结果均为合成数据。', status: missing ? 'unavailable' : 'captured', sha256: missing ? null : 'd'.repeat(64) },
    ],
    ledger: [
      { id: 'goal-settings', kind: 'goal', text: '统一设置页交互，保留用户主动保存。', source_ids: ['request'], status: 'active', supersedes: [], critical: false },
      { id: 'save-manual', kind: 'constraint', text: '保存方式保持 manual。', source_ids: ['request'], status: 'active', supersedes: ['save-auto'], critical: true },
      { id: 'save-auto', kind: 'constraint', text: '旧方案：切换为自动保存。', source_ids: ['request'], status: 'superseded', supersedes: [], critical: false },
      { id: 'feedback', kind: 'constraint', text: '保留保存完成的反馈。', source_ids: ['request'], status: 'active', supersedes: [], critical: false },
    ],
    probes: [
      { id: 'check-save', item_id: 'save-manual', source_id: 'settings', rule: 'contains', expected: "saveMode: 'manual'", result: missing ? 'unknown' : passed ? 'pass' : 'fail',
        detail: missing ? '所选文件不可读，未执行文字检查。' : passed ? '所选版本包含指定的 manual 保存配置。' : '所选版本未找到指定的 manual 保存配置。' },
      { id: 'check-feedback', item_id: 'feedback', source_id: 'settings', rule: 'contains', expected: 'savedFeedback: true', result: missing ? 'unknown' : 'pass',
        detail: missing ? '所选文件不可读，未执行文字检查。' : '所选版本包含指定的反馈配置。' },
    ],
    observations: scenario === 'failed' ? [{ id: 'old-plan', item_id: 'save-manual', kind: 'stale_fact', summary: '演示记录：已否决的自动保存方案似乎再次成为前提；该归因尚待核验。', status: 'open', recurrence: 'after_correction', source_ids: ['request', 'settings'] }] : [],
  }
}

export function demoSessions(scenario: DemoScenario, now = Date.now()): ContextSnapshot[] {
  return [
    { id: DEMO_PRIMARY_ID, title: '设置页交互优化', model: '示例模型', source: 'demo', observedAt: now,
      turnId: 'demo-turn-30', usedTokens: 196_000, windowTokens: 200_000, compactions: 8,
      report: demoReport(scenario, now) },
    { id: DEMO_BACKGROUND_ID, title: '设置页验证（后台）', model: '示例模型', source: 'demo', observedAt: now,
      turnId: 'demo-turn-b', usedTokens: 48_000, windowTokens: 200_000, compactions: 0,
      report: { ...demoReport('passed', now), session_id: DEMO_BACKGROUND_ID, turn_id: 'demo-turn-b' } },
  ]
}

export function legacyDemoAssessment(now = Date.now()): SemanticAssessment {
  return {
    schema_version: 1, source: 'codex-skill-review', session_id: DEMO_PRIMARY_ID, turn_id: 'demo-turn-30',
    reviewed_at_ms: now, compactions_observed: 8, coverage: 'sufficient',
    current_goal: '旧版演示目标。', next_step: '旧版演示下一步。', review_note: '旧版评估只能回顾。', signals: [],
  }
}
