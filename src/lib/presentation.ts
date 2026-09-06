import { evaluateContext, type ContextSnapshot } from './context'

export type RiskTone = 'unknown' | 'aligned' | 'review' | 'deviation'

/** A visual vocabulary for evidence, never a numeric estimate of semantic contamination. */
export function presentRisk(snapshot: ContextSnapshot | undefined, now = Date.now()) {
  const health = evaluateContext(snapshot, now)
  const active = new Set(health.report?.ledger.filter(item => item.status === 'active').map(item => item.id))
  const observations = health.report?.observations.filter(item => item.status === 'open' && active.has(item.item_id)).length ?? 0
  let tone: RiskTone = 'unknown'
  let value = '尚未评估'
  let detail = '先收集当前目标、约束与下一步的依据。'
  if (health.checks.failed > 0) {
    tone = 'deviation'; value = '发现检查偏差'
    detail = `${health.checks.failed} 项文件条件未满足，先核对具体改动。`
  } else if (observations > 0) {
    tone = 'review'; value = '疑点待核验'
    detail = `${observations} 条记录尚未独立验证。`
  } else if (health.level === 'healthy') {
    tone = 'aligned'; value = '所列依据一致'
    detail = '采集时所列检查通过，仍以本次范围为限。'
  } else if (health.report) {
    value = '证据待补齐'
    detail = health.checks.unknown ? `${health.checks.unknown} 项检查仍未核验。` : '仍有前提需要确认。'
  }
  return { tone, value, detail, observations, health }
}

function count(value: number | null | undefined) {
  return typeof value === 'number' && Number.isSafeInteger(value) && value >= 0
}

/** Native hooks do not carry token occupancy. A report or legacy review cannot supply it. */
export function presentCapacity(snapshot: ContextSnapshot | undefined, now = Date.now()) {
  const available = snapshot?.source === 'demo' && count(snapshot.usedTokens)
    && count(snapshot.windowTokens) && snapshot.windowTokens! > 0 && snapshot.usedTokens! <= snapshot.windowTokens!
    && Number.isSafeInteger(snapshot.observedAt) && snapshot.observedAt >= 0 && snapshot.observedAt <= now + 60_000
  if (!available) return { ratio: null, percent: null, used: null, total: null, source: '未接入用量', stale: false, pressure: 'unknown' as const }
  const ratio = snapshot.usedTokens! / snapshot.windowTokens!
  return { ratio, percent: Math.round(ratio * 100), used: snapshot.usedTokens, total: snapshot.windowTokens,
    source: '演示用量', stale: now - snapshot.observedAt > 5 * 60_000,
    pressure: ratio >= .9 ? 'high' as const : ratio >= .75 ? 'elevated' as const : 'normal' as const }
}

export function presentCompactions(snapshot: ContextSnapshot | undefined) {
  if (snapshot?.source === 'demo' && count(snapshot.compactions)) return { value: String(snapshot.compactions), detail: '演示次数' }
  return { value: '未记录', detail: snapshot?.lastEvent === 'PostCompact' ? '最近收到压缩完成事件，累计次数未知' : '累计次数未接入' }
}

export function formatTokens(value: number | null) {
  if (value === null) return '未知'
  return value >= 1000 ? `${(value / 1000).toLocaleString('zh-CN', { maximumFractionDigits: 1 })}k` : String(value)
}
