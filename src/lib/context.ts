export type HealthLevel = 'healthy' | 'watch' | 'handoff' | 'unknown'
export type TelemetrySource = 'demo' | 'codex-hook' | 'codex-skill-review'
export type SignalKind = 'goal_drift' | 'constraint_loss' | 'decision_conflict' | 'stale_fact' | 'repeated_work'

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

/** A scoped review, not a measurement of the model's internal state or entropy. */
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
  // Diagnostic values are deliberately excluded from the decision rule.
  usedTokens: number | null
  windowTokens: number | null
  compactions: number | null
  lastEvent?: string
  assessment?: SemanticAssessment | null
}

export interface ContextHealth {
  level: HealthLevel
  label: string
  headline: string
  description: string
  reasons: { title: string; description: string }[]
  actionable: SemanticSignal[]
  reviewedAt: number | null
}

export const SIGNAL_LABELS: Record<SignalKind, string> = {
  goal_drift: '目标串线', constraint_loss: '约束遗漏', decision_conflict: '决策冲突',
  stale_fact: '旧结论回流', repeated_work: '重复返工',
}

export const DEFAULT_POLICY = { minimumCompactions: 2, reviewMaxAgeMs: 20 * 60 * 1000 }

function unknown(description: string, reviewedAt: number | null = null): ContextHealth {
  return {
    level: 'unknown', label: '等待评估', headline: '先确认这段思路是否清楚',
    description, reasons: [], actionable: [], reviewedAt,
  }
}

function references(signal: SemanticSignal) {
  return new Set(signal.evidence.map(item => item.ref.trim()).filter(Boolean))
}

/** Two descriptions of the same observation are not independent failures. */
function independent(a: SemanticSignal, b: SemanticSignal) {
  if (a.id === b.id || a.kind === b.kind) return false
  const left = references(a), right = references(b)
  return [...left].some(ref => !right.has(ref)) && [...right].some(ref => !left.has(ref))
}

export function evaluateContext(snapshot: ContextSnapshot | undefined, now = Date.now(), policy = DEFAULT_POLICY): ContextHealth {
  if (!snapshot) return unknown('固定正在关注的会话，再检查当前目标、有效约束和下一步。')
  const review = snapshot.assessment
  if (!review) return unknown('尚无语义评估。长度和压缩次数无法说明后续执行是否受干扰。')
  if (review.schema_version !== 1 || review.source !== 'codex-skill-review' || review.session_id !== snapshot.id) {
    return unknown('评估与当前固定会话不匹配，等待新的评估。')
  }
  const stamp = review.reviewed_at_ms
  if (!Number.isSafeInteger(stamp) || stamp < 0 || stamp > now + 60_000 || now - stamp > policy.reviewMaxAgeMs) {
    return unknown('这份评估已过期或时间无法验证，需要结合当前进展重新检查。')
  }
  if (!Number.isSafeInteger(snapshot.observedAt) || snapshot.observedAt < 0 || snapshot.observedAt > now + 60_000) {
    return unknown('会话事件时间无法验证，需要结合当前进展重新检查。', stamp)
  }
  if (snapshot.turnId && review.turn_id && snapshot.turnId !== review.turn_id) {
    return unknown('会话已进入另一轮。上一轮评估不能直接代表当前执行状态。', stamp)
  }
  if (snapshot.observedAt > stamp) {
    return unknown('评估后又收到会话事件。这份报告只反映评估当时，请结合当前进展复查。', stamp)
  }
  const count = review.compactions_observed
  if (count !== null && (!Number.isSafeInteger(count) || count < 0 || count > 10_000)) {
    return unknown('压缩记录无法验证，不能据此建议换会话。')
  }
  const open = review.signals.filter(signal => signal.status === 'open' && signal.affects_next_step)
  const actionable = open.filter(signal => signal.confidence !== 'low' && references(signal).size >= 2)
  const strong = actionable.filter(signal => signal.after_compaction && signal.confidence === 'high')
  const recurring = strong.filter(signal => signal.recurrence === 'after_correction' && references(signal).size >= 3)
  const corroborated = recurring.some(a => strong.some(b => independent(a, b)))
  const recommend = review.coverage === 'sufficient' && count !== null && count >= policy.minimumCompactions && corroborated
  if (!actionable.length && (review.coverage !== 'sufficient' || open.length)) {
    return unknown('可核对的证据还不够。先补齐目标、约束和最近执行记录，再判断是否需要交接。', stamp)
  }
  const level = recommend ? 'handoff' : actionable.length ? 'watch' : 'healthy'
  const copy = {
    healthy: {
      label: '脉络清晰', headline: '多次压缩，也可以继续专注',
      description: '本次评估中，当前目标与下一步保持一致，没有发现仍在干扰执行的信息冲突。',
    },
    watch: {
      label: '需要留意', headline: '先理清这一点，再继续',
      description: review.coverage === 'partial'
        ? '有限记录中发现待核对的执行偏差。先澄清具体问题，再补一轮评估。'
        : '已发现影响下一步的疑点。先核对有效约束、澄清冲突，再观察能否恢复清晰。',
    },
    handoff: {
      label: '建议新开', headline: '这段思路，适合重新开始',
      description: '多次压缩后，纠正过的问题仍在回流，且有不同执行偏差相互印证。建议整理有效信息，在新会话继续。',
    },
  }[level]
  return {
    level, ...copy, actionable, reviewedAt: stamp,
    reasons: actionable.map(signal => ({ title: SIGNAL_LABELS[signal.kind], description: signal.summary })),
  }
}

/** Never fall back to the most recently active session. */
export function resolvePinned(sessions: ContextSnapshot[], pinnedId: string | null) {
  return pinnedId ? sessions.find(session => session.id === pinnedId) : undefined
}

export function createHandoffTemplate(snapshot: ContextSnapshot | undefined, now = Date.now()): string {
  const health = evaluateContext(snapshot, now)
  const review = health.level !== 'unknown' ? snapshot?.assessment : null
  return [
    `# 干净交接 · ${snapshot?.title ?? '待填写的任务'}`,
    '', '## 当前唯一目标',
    review ? `${review.current_goal}\n[来自最近评估，请先核对是否仍有效]` : '[填写当前仍要完成的一件事]',
    '', '## 已确认的事实与结果', '[仅保留已验证结论，附文件或证据位置]',
    '', '## 仍然有效的约束', '[以最新确认的约束为准；排除已经作废的要求]',
    '', '## 下一步',
    review ? `${review.next_step}\n[执行前核对前置条件]` : '[写明下一项可执行动作及验收条件]',
    '', '## 交接前须澄清的疑点（不能当作已确认事实）',
    ...health.actionable.map(signal => `- ${SIGNAL_LABELS[signal.kind]}：${signal.summary}`),
    '[解决冲突后写入有效结论；不把互相矛盾的旧说法一起带过去]',
    '', '## 明确不再沿用的信息', '[列出已废弃的方案、旧事实和无关目标；未核实的信息继续标为未知]',
    '', '这是待填写的交接模板。填写并核实后，再复制到新会话。',
  ].join('\n')
}

export function createReviewPrompt(sessionId: string | null): string {
  return [
    '请使用 $context-health 评估当前会话的压缩后语义完整性。',
    sessionId ? `目标会话 ID：${sessionId}。请先核对身份，不按最近活动猜测。` : '先取得并核对当前会话的精确 ID；不能取得时只在本轮答复中报告，不绑定其他会话。',
    '检查当前目标、仍有效的约束与最近执行行为。压缩次数和上下文长度本身不构成换会话理由。',
    '重点找压缩后仍未解决、影响下一步、纠正后再次出现的约束遗漏、旧结论回流、目标串线或重复返工。',
    '逐项给出来源位置，区分已解决问题、正常需求调整、工具故障和证据不足；不要生成虚构的熵值或评分。',
    '将最小化的结构化评估保存在 Context Orb 的本地评估目录，供悬浮球显示；不保存完整对话或凭证。',
    '无需创建、中断、压缩或关闭任何会话。',
  ].join('\n')
}
