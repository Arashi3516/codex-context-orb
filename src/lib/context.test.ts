import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import { createHandoffTemplate, createReviewPrompt, evaluateContext, resolvePinned } from './context'
import type { ContextSnapshot } from './context'
import { demoReport, demoSessions, legacyDemoAssessment } from './demo'
import { isEvidenceReport } from './evidence'

const now = 2_000_000
function snapshot(): ContextSnapshot { return demoSessions('passed', now)[0] }

describe('as-of evidence decisions', () => {
  it('uses the same valid fixture as the collector and native reader', () => {
    const fixture: unknown = JSON.parse(readFileSync(new URL('../../plugins/codex-context-orb/scripts/fixtures/evidence-report-valid.json', import.meta.url), 'utf-8'))
    expect(isEvidenceReport(fixture)).toBe(true)
    if (!isEvidenceReport(fixture)) throw new Error('Invalid shared fixture')
    expect(evaluateContext({ ...snapshot(), id: fixture.session_id, report: fixture }, fixture.reviewed_at_ms).level).toBe('healthy')
  })
  it('ignores capacity, compaction count and elapsed minutes for a scoped receipt', () => {
    for (const compactions of [null, 0, 1, 10, 10_000]) {
      const item = { ...snapshot(), compactions, usedTokens: 199_999, windowTokens: 200_000 }
      const result = evaluateContext(item, now + 21 * 60_000)
      expect(result.level).toBe('healthy')
      expect(result.label).toBe('所列检查通过')
      expect(result.notices.join('')).toContain('截至采集时点')
      expect(result.restartBenefit).toBe('not_evaluated')
    }
  })
  it('one failed check is visible without repeated errors, compaction or a second signal', () => {
    const item = { ...snapshot(), compactions: 0, report: demoReport('failed', now) }
    item.report.observations = []
    const result = evaluateContext(item, now)
    expect(result.level).toBe('watch')
    expect(result.checks).toEqual({ passed: 1, failed: 1, unknown: 0 })
    expect(result.restartBenefit).toBe('not_evaluated')
  })
  it('shows a failure alongside missing critical evidence without claiming completeness', () => {
    const item = snapshot()
    item.report = demoReport('failed', now)
    item.report.scope.unknowns = ['还有一个授权前提未确认。']
    const result = evaluateContext(item, now)
    expect(result.level).toBe('watch')
    expect(result.unresolved).toContain('还有一个授权前提未确认。')
  })
  it('missing files produce unknown checks, never passing empty text checks', () => {
    const item = { ...snapshot(), report: demoReport('unknown', now) }
    expect(evaluateContext(item, now).level).toBe('unknown')
    expect(evaluateContext(item, now).checks.unknown).toBe(2)
  })
  it('does not infer a critical constraint from an unrelated passing check', () => {
    const item = snapshot()
    item.report!.probes = item.report!.probes.filter(probe => probe.item_id !== 'save-manual')
    expect(evaluateContext(item, now).level).toBe('unknown')
    expect(evaluateContext(item, now).unresolved.join('')).toContain('缺少实际检查')
  })
  it('manual probes always remain unknown', () => {
    const item = snapshot()
    item.report!.probes[0] = { ...item.report!.probes[0], rule: 'manual', source_id: null, expected: '', result: 'unknown', detail: '需要人工交互核验。' }
    expect(evaluateContext(item, now).level).toBe('unknown')
  })
  it('retains unverified commentary as a gap rather than independent evidence', () => {
    const item = snapshot()
    item.report!.observations = demoReport('failed', now).observations
    const result = evaluateContext(item, now)
    expect(result.level).toBe('unknown')
    expect(result.checks.failed).toBe(0)
    expect(result.unresolved.join('')).toContain('独立核验')
  })
  it('superseded entries do not create missing checks for the current scope', () => {
    const item = snapshot()
    item.report!.ledger.find(atom => atom.id === 'save-auto')!.critical = true
    expect(evaluateContext(item, now).level).toBe('healthy')
  })
  it.each(['origin', 'coverage', 'hypothesis'] as const)('does not hide %s uncertainty behind passing checks', condition => {
    const item = snapshot()
    if (condition === 'origin') item.report!.scope.origin = 'unknown'
    if (condition === 'coverage') item.report!.scope.coverage = 'partial'
    if (condition === 'hypothesis') item.report!.ledger.push({ id: 'unverified', kind: 'fact', text: '尚未证实的前提。', source_ids: ['request'], status: 'hypothesis', supersedes: [], critical: false })
    expect(evaluateContext(item, now).level).toBe('unknown')
  })
  it('newer Stop and turn activity do not rewrite the historical receipt', () => {
    const item = { ...snapshot(), lastEvent: 'Stop', turnId: 'another-turn', observedAt: now + 1 }
    const result = evaluateContext(item, now + 1)
    expect(result.level).toBe('healthy')
    expect(result.reviewedAt).toBe(now)
    expect(result.notices.join('')).toContain('新的会话活动')
    expect(result.notices.join('')).toContain('另一轮')
  })
  it('rejects an identity mismatch and a report from an invalid future', () => {
    expect(evaluateContext({ ...snapshot(), id: 'different-session' }, now).level).toBe('unknown')
    const item = snapshot()
    item.report!.reviewed_at_ms = now + 60_001
    expect(evaluateContext(item, now).level).toBe('unknown')
  })
  it('legacy v1 reports never produce active advice, even with high confidence', () => {
    const item = { ...snapshot(), report: null, assessment: legacyDemoAssessment(now) }
    item.assessment.signals = [{ id: 'old', kind: 'constraint_loss', summary: '旧版主观记录', status: 'open', after_compaction: true, affects_next_step: true, recurrence: 'after_correction', confidence: 'high', evidence: [{ ref: 'r1', note: '说明一' }, { ref: 'r2', note: '说明二' }, { ref: 'r3', note: '说明三' }] }]
    expect(evaluateContext(item, now).level).toBe('unknown')
    expect(evaluateContext(item, now).report).toBeNull()
  })
  it('keeps a deliberately selected session instead of following recency', () => {
    const items = demoSessions('passed', now)
    items[1].observedAt = now + 100
    expect(resolvePinned(items, items[0].id)).toBe(items[0])
    expect(resolvePinned(items, null)).toBeUndefined()
    expect(resolvePinned(items, 'missing')).toBeUndefined()
  })
})

describe('evidence contract at the renderer boundary', () => {
  it('rejects impossible captured paths while preserving unknown prohibited sources', () => {
    for (const ref of ['/tmp/file.txt', '../outside.txt', 'C:\\file.txt', 'src//file.txt', 'NUL.txt', 'file.', '.codex/session.json', '.ENV.local']) {
      const report = demoReport('passed', now)
      report.sources[1].ref = ref
      expect(isEvidenceReport(report), ref).toBe(false)
    }
    const unavailable = demoReport('unknown', now)
    unavailable.sources[1].ref = '.env'
    expect(isEvidenceReport(unavailable)).toBe(true)
    expect(evaluateContext({ ...snapshot(), report: unavailable }, now).level).toBe('unknown')
  })
  it.each([
    ['duplicate source', (r: ReturnType<typeof demoReport>) => r.sources.push({ ...r.sources[0] })],
    ['missing reference', (r: ReturnType<typeof demoReport>) => { r.ledger[0].source_ids = ['missing'] }],
    ['active supersession target', (r: ReturnType<typeof demoReport>) => { r.ledger[2].status = 'active' }],
    ['cycle', (r: ReturnType<typeof demoReport>) => { r.ledger[2].supersedes = ['save-auto'] }],
    ['unsupported metric', (r: ReturnType<typeof demoReport>) => Object.assign(r.scope, { entropy: 0.9 })],
    ['empty literal', (r: ReturnType<typeof demoReport>) => { r.probes[0].expected = '' }],
    ['manual pass', (r: ReturnType<typeof demoReport>) => { r.probes[0] = { ...r.probes[0], rule: 'manual', source_id: null, expected: '', result: 'pass' } }],
    ['uncaptured pass', (r: ReturnType<typeof demoReport>) => { r.sources[1].status = 'unavailable'; r.sources[1].sha256 = null }],
    ['statement forged as file', (r: ReturnType<typeof demoReport>) => { r.sources[0].status = 'captured'; r.sources[0].sha256 = 'a'.repeat(64) }],
    ['hash/result disagreement', (r: ReturnType<typeof demoReport>) => { r.probes[0].rule = 'sha256'; r.probes[0].expected = 'f'.repeat(64) }],
  ] as const)('rejects %s', (_name, mutate) => {
    const report = demoReport('passed', now)
    mutate(report)
    expect(isEvidenceReport(report)).toBe(false)
    expect(evaluateContext({ ...snapshot(), report }, now).report).toBeNull()
  })
  it('supports Unicode scalar text while rejecting isolated surrogate characters', () => {
    const report = demoReport('passed', now)
    report.scope.next_step = '核验保存入口 🧭'
    expect(isEvidenceReport(report)).toBe(true)
    report.scope.next_step = '\ud800'
    expect(isEvidenceReport(report)).toBe(false)
  })
})

describe('reuse without an unvalidated restart claim', () => {
  it('does not reintroduce superseded observations as unresolved current work', () => {
    const item = snapshot()
    item.report!.observations = [{ id: 'old-observation', item_id: 'save-auto', kind: 'decision_conflict', summary: '旧自动保存方案的历史疑点。', status: 'open', recurrence: 'once', source_ids: ['request'] }]
    expect(evaluateContext(item, now).level).toBe('healthy')
    expect(createHandoffTemplate(item, now)).not.toContain('旧自动保存方案的历史疑点')
  })
  it('carries declared decisions and unresolved hypotheses without promoting them to facts', () => {
    const item = snapshot()
    item.report!.ledger.push(
      { id: 'current-decision', kind: 'decision', text: '继续使用当前数据接口。', source_ids: ['request'], status: 'active', supersedes: [], critical: false },
      { id: 'pending-assumption', kind: 'fact', text: '接口支持撤销仍需验证。', source_ids: ['request'], status: 'hypothesis', supersedes: [], critical: true },
    )
    const output = createHandoffTemplate(item, now)
    expect(output).toContain('决策：继续使用当前数据接口。')
    expect(output).toContain('待验证假设：接口支持撤销仍需验证。')
    expect(output).toContain('未必已独立核验')
    expect(output).toContain('saveMode:')
    expect(output).toContain('src/settings.ts')
  })
  it('builds the same task-state brief for repair or an optional new conversation', () => {
    const output = createHandoffTemplate({ ...snapshot(), report: demoReport('failed', now) }, now)
    expect(output).toContain('当前会话内纠正')
    expect(output).toContain('新开收益尚未评估')
    expect(output).toContain('FAIL')
    expect(output).toContain('## 来源版本')
    expect(output).toContain('d'.repeat(64))
    expect(output).toContain('旧方案：切换为自动保存。')
  })
  it('does not promote legacy content into a verified brief', () => {
    const output = createHandoffTemplate({ ...snapshot(), report: null, assessment: legacyDemoAssessment(now) }, now)
    expect(output).not.toContain('旧版演示目标')
    expect(output).toContain('尚无可验证的 v2 报告')
  })
  it('asks for actual collection and preserves the no-transcript/no-extra-model scope', () => {
    const prompt = createReviewPrompt('exact-session')
    expect(prompt).toContain('exact-session')
    expect(prompt).toContain('evidence_review.py collect')
    expect(prompt).toContain('不手写 pass/fail')
    expect(prompt).toContain('不读取私有 Codex 转录')
  })
})
