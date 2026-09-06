import { ITEM_LABELS, RULE_LABELS, isEvidenceReport, type EvidenceReport, type ProbeResult, type SignalKind } from './evidence'

export type HealthLevel = 'healthy' | 'watch' | 'unknown'
export type TelemetrySource = 'demo' | 'codex-hook' | 'codex-skill-review' | 'codex-evidence-review'
export type { SignalKind } from './evidence'

/** Legacy v1 is kept for historical viewing only. Its heuristics no longer issue advice. */
export interface SemanticSignal {
  id: string
  kind: SignalKind
  summary: string
  status: 'open' | 'resolved'
  after_compaction: boolean
  affects_next_step: boolean
  recurrence: 'once' | 'after_correction'
  confidence: 'low' | 'medium' | 'high'
  evidence: { ref: string; note: string }[]
}
export interface SemanticAssessment {
  schema_version: 1
  source: 'codex-skill-review'
  session_id: string
  turn_id: string | null
  reviewed_at_ms: number
  compactions_observed: number | null
  coverage: 'sufficient' | 'partial'
  current_goal: string
  next_step: string
  review_note: string
  signals: SemanticSignal[]
}
export interface ContextSnapshot {
  id: string
  title: string
  source: TelemetrySource
  model: string | null
  observedAt: number
  turnId?: string | null
  usedTokens: number | null
  windowTokens: number | null
  compactions: number | null
  lastEvent?: string
  assessment?: SemanticAssessment | null
  report?: EvidenceReport | null
}
export interface ContextHealth {
  level: HealthLevel
  label: string
  headline: string
  description: string
  report: EvidenceReport | null
  reviewedAt: number | null
  checks: { passed: number; failed: number; unknown: number }
  unresolved: string[]
  notices: string[]
  restartBenefit: 'not_evaluated'
}

export const SIGNAL_LABELS: Record<SignalKind, string> = {
  goal_drift: '目标偏离', constraint_loss: '约束遗漏', decision_conflict: '决策冲突',
  stale_fact: '旧结论回流', repeated_work: '无效重复',
}

function unknown(description: string): ContextHealth {
  return {
    level: 'unknown', label: '证据待补齐', headline: '先核对下一步的依据', description,
    report: null, reviewedAt: null, checks: { passed: 0, failed: 0, unknown: 0 },
    unresolved: [], notices: [], restartBenefit: 'not_evaluated',
  }
}

/** A result is permanently scoped to collection time; it never certifies a live model context. */
export function evaluateContext(snapshot: ContextSnapshot | undefined, now = Date.now()): ContextHealth {
  if (!snapshot) return unknown('手动固定一个会话，再收集当前目标、有效约束与下一步检查。')
  const report = snapshot.report
  if (!report) return unknown(snapshot.assessment
    ? '旧版评估仅供回顾。请收集带来源与实际检查结果的新报告。'
    : '尚无证据报告。先声明检查范围，再核验明确选定的文件。')
  if (!isEvidenceReport(report) || report.session_id !== snapshot.id) return unknown('报告身份或证据引用无法验证，请重新收集。')
  if (!Number.isSafeInteger(now) || now < 0 || report.reviewed_at_ms > now + 60_000) return unknown('报告时间无法验证，请检查本地时钟。')
  const sources = new Map(report.sources.map(source => [source.id, source]))
  const active = report.ledger.filter(item => item.status === 'active')
  const probes = report.probes
  const checks = {
    passed: probes.filter(probe => probe.result === 'pass').length,
    failed: probes.filter(probe => probe.result === 'fail').length,
    unknown: probes.filter(probe => probe.result === 'unknown').length,
  }
  const unresolved = [...report.scope.unknowns]
  if (report.scope.origin === 'unknown') unresolved.push('会话来源尚未确认。')
  if (report.scope.coverage === 'partial') unresolved.push('声明的任务范围仍不完整。')
  if (active.some(item => item.source_ids.some(id => sources.get(id)?.status === 'unavailable'))) unresolved.push('有效条目的来源暂不可读。')
  if (report.ledger.some(item => item.status === 'hypothesis')) unresolved.push('账本中还有尚未验证的假设。')
  if (report.observations.some(item => item.status === 'open' && active.some(atom => atom.id === item.item_id))) unresolved.push('评估者记录的疑点仍需独立核验。')
  const uncovered = active.filter(item => (item.kind === 'constraint' || item.critical)
    && !probes.some(probe => probe.item_id === item.id && probe.rule !== 'manual' && probe.result !== 'unknown'))
  if (uncovered.length) unresolved.push(`${uncovered.length} 项约束或关键条目缺少实际检查。`)
  if (checks.unknown) unresolved.push(`${checks.unknown} 项检查尚无可核验结果。`)
  if (!checks.passed && !checks.failed) unresolved.push('还没有完成任何文件检查。')
  const level: HealthLevel = checks.failed ? 'watch' : unresolved.length ? 'unknown' : 'healthy'
  const copy = {
    healthy: { label: '所列检查通过', headline: '这一步，有据可循', description: '采集时，声明范围内的检查均已通过。原始要求由本次评估整理，文件随后变化时需要重新检查。' },
    watch: { label: '检查未通过', headline: '先处理这个具体偏差', description: '所选文件没有满足声明的检查条件。先核对来源与改动，再决定如何继续。' },
    unknown: { label: '证据待补齐', headline: '还有前提需要确认', description: '目前不能判断全部所列检查。保留已获得的结果，并补齐下面的未知项。' },
  }[level]
  const notices = ['截至采集时点的结果，不是当前模型上下文的实时保证。']
  if (snapshot.lastEvent && Number.isSafeInteger(snapshot.observedAt) && snapshot.observedAt > report.reviewed_at_ms) {
    notices.push('报告之后收到新的会话活动；执行前请核对范围与文件版本。')
  }
  if (snapshot.turnId && report.turn_id && snapshot.turnId !== report.turn_id) notices.push('会话活动属于另一轮；这份报告仍仅对应原检查范围。')
  return { level, ...copy, report, reviewedAt: report.reviewed_at_ms, checks,
    unresolved: [...new Set(unresolved)], notices, restartBenefit: 'not_evaluated' }
}

export function resolvePinned(sessions: ContextSnapshot[], pinnedId: string | null) {
  return pinnedId ? sessions.find(session => session.id === pinnedId) : undefined
}

export function describeProbe(report: EvidenceReport, probe: ProbeResult) {
  return report.ledger.find(item => item.id === probe.item_id)?.text ?? '未知条目'
}

/** The same reviewed state can be reused in the current conversation or a new one. */
export function createHandoffTemplate(snapshot: ContextSnapshot | undefined, now = Date.now()): string {
  const health = evaluateContext(snapshot, now)
  const report = health.report
  const active = report?.ledger.filter(item => item.status === 'active') ?? []
  const refs = (ids: string[]) => ids.map(id => report?.sources.find(source => source.id === id)?.ref ?? id).join('；')
  return [
    '# 下一步任务简报', '',
    '可用于当前会话内纠正，也可供你选择在新会话继续。新开收益尚未评估。',
    report ? `采集时间：${new Date(report.reviewed_at_ms).toISOString()}\n报告版本：${report.report_id}` : '尚无可验证的 v2 报告，请先补齐以下内容。',
    '', '## 当前声明的目标',
    ...active.filter(item => item.id === report?.scope.goal_id).map(item => `${item.text} [来源：${refs(item.source_ids)}]`),
    ...(!report ? ['[待确认]'] : []),
    '', '## 声明的有效约束（执行前核对原始要求）',
    ...active.filter(item => item.kind === 'constraint').map(item => `- ${item.text} [来源：${refs(item.source_ids)}]`),
    '', '## 声明的事实、决策与进展（未必已独立核验）',
    ...active.filter(item => ['fact', 'decision', 'progress'].includes(item.kind)).map(item => `- ${ITEM_LABELS[item.kind]}：${item.text} [来源：${refs(item.source_ids)}]`),
    '', '## 本次检查结果（文字条件不证明行为正确）',
    ...(report?.probes.map(probe => `- ${probe.result.toUpperCase()}：${describeProbe(report, probe)}；${RULE_LABELS[probe.rule]}${probe.expected ? ` ${JSON.stringify(probe.expected)}` : ''}；来源：${probe.source_id ? refs([probe.source_id]) : '未执行文件检查'}；${probe.detail}`) ?? ['[待收集]']),
    '', '## 来源版本',
    ...(report?.sources.filter(source => source.kind === 'artifact').map(source => `- ${source.ref}：${source.sha256 ?? '不可读 / 未验证'}`) ?? []),
    '', '## 未知与待处理',
    ...health.unresolved.map(item => `- ${item}`),
    ...(report?.ledger.filter(item => item.status === 'hypothesis').map(item => `- 待验证假设：${item.text} [来源：${refs(item.source_ids)}]`) ?? []),
    ...(report?.observations.filter(item => item.status === 'open' && report.ledger.find(atom => atom.id === item.item_id)?.status !== 'superseded').map(item => `- 评估者记录，尚待核验：${item.summary} [来源：${refs(item.source_ids)}]`) ?? []),
    '', '## 不再沿用',
    ...(report?.ledger.filter(item => item.status === 'superseded').map(item => `- ${item.text} [来源：${refs(item.source_ids)}]`) ?? []),
    '', '## 下一步', report?.scope.next_step ?? '[待填写]',
    '', '这是待核对的任务简报。文件、权限或目标变化后，重新验证相关前提。',
  ].join('\n')
}

export function createReviewPrompt(sessionId: string | null): string {
  return [
    '请使用 $context-health 收集当前任务的来源账本与下一步检查报告。',
    sessionId ? `目标会话 ID：${sessionId}。请先核对身份，不按最近活动猜测。` : '先取得并核对当前会话的精确 ID；不能取得时只报告缺口，不绑定其他会话。',
    '从本次可见的明确要求整理目标、约束、已作废决策和未知项，给出来源位置。声明的来源不等于宿主独立验证。',
    '仅检查我当前任务中明确选定的工作区文件；为下一步列出可核验的文字或文件哈希条件。无法客观检查的条件设为 manual / unknown。',
    '使用 evidence_review.py collect 生成实际检查结果，不手写 pass/fail，不读取私有 Codex 转录或凭证，也不调用额外模型。',
    '报告只反映采集时点；压缩次数、引用数量和模型自报信心不决定健康或新开建议。',
    '把有效状态整理为可在当前会话复用的简报；新开收益保持未评估。不要自动创建、中断或关闭会话。',
  ].join('\n')
}
