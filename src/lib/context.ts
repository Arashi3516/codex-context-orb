export type HealthLevel = 'healthy' | 'watch' | 'handoff' | 'unknown'
export type TelemetrySource = 'demo' | 'codex-hook'

export interface ContextSnapshot {
  id: string
  title: string
  source: TelemetrySource
  model: string | null
  observedAt: number
  usedTokens: number | null
  windowTokens: number | null
  compactions: number | null
  lastEvent?: string
}

export interface HealthReason {
  title: string
  description: string
}

export interface ContextHealth {
  level: HealthLevel
  label: string
  headline: string
  description: string
  percent: number | null
  reasons: HealthReason[]
}

export interface HealthPolicy {
  cautionPercent: number
  handoffPercent: number
  staleAfterMs: number
}

export const DEFAULT_POLICY: HealthPolicy = {
  cautionPercent: 75,
  handoffPercent: 90,
  staleAfterMs: 5 * 60 * 1000,
}

function unknown(description: string): ContextHealth {
  return {
    level: 'unknown', label: '等待数据', headline: '先确认正在关注的会话',
    description, percent: null, reasons: [],
  }
}

export function evaluateContext(
  snapshot: ContextSnapshot | undefined,
  now = Date.now(),
  policy = DEFAULT_POLICY,
): ContextHealth {
  if (!snapshot) return unknown('选择并固定一个会话，避免其他后台任务干扰提醒。')
  if (!Number.isFinite(snapshot.observedAt) || snapshot.observedAt > now + 60_000) {
    return unknown('数据时间无法验证，暂不判断上下文状态。')
  }
  if (now - snapshot.observedAt > policy.staleAfterMs) {
    return unknown('数据已超过 5 分钟未更新，等待新的会话事件。')
  }
  const { usedTokens, windowTokens } = snapshot
  if (usedTokens === null || windowTokens === null) {
    return unknown('会话已固定。当前接入仅提供事件，尚未提供上下文用量。')
  }
  if (!Number.isFinite(usedTokens) || !Number.isFinite(windowTokens) ||
      usedTokens < 0 || windowTokens <= 0 || usedTokens > windowTokens) {
    return unknown('上下文用量无法验证，暂不判断是否需要新会话。')
  }

  const percent = (usedTokens / windowTokens) * 100
  const compactions = snapshot.compactions
  const level: HealthLevel = percent >= policy.handoffPercent ? 'handoff'
    : percent >= policy.cautionPercent || (compactions !== null && compactions >= 2) ? 'watch'
    : 'healthy'

  const copy = {
    healthy: {
      label: '余量充足', headline: '继续专注，余量还很充足',
      description: '当前容量压力较低。保持一个明确目标，让这段工作自然完成。',
    },
    watch: {
      label: '留意余量', headline: '为下一段思路，留一点余量',
      description: '可以继续当前步骤。进入新阶段前，留下一份简短交接会更从容。',
    },
    handoff: {
      label: '建议交接', headline: '下一步，适合从新会话开始',
      description: '上下文已接近容量上限。先保存已确认的进展，再开始下一段工作。',
    },
  }[level]
  const reasons: HealthReason[] = [{
    title: `上下文已使用 ${Math.round(percent)}%`,
    description: `本次上下文 ${formatTokens(usedTokens)} / ${formatTokens(windowTokens)} tokens；不是历史累计用量。`,
  }]
  if (compactions !== null && compactions >= 2) reasons.push({
    title: `这段会话已压缩 ${compactions} 次`,
    description: '多次压缩是检查交接时机的信号，本身不能证明会话内容混乱。',
  })
  return { level, ...copy, percent, reasons }
}

export function formatTokens(value: number | null): string {
  if (value === null) return '—'
  if (value >= 1000) return `${(value / 1000).toFixed(value % 1000 === 0 ? 0 : 1)}k`
  return String(value)
}

/** Never fall back to the most recently active session. */
export function resolvePinned(sessions: ContextSnapshot[], pinnedId: string | null) {
  return pinnedId ? sessions.find(session => session.id === pinnedId) : undefined
}

export function createHandoffTemplate(snapshot: ContextSnapshot | undefined): string {
  const title = snapshot?.title ?? '待填写的任务'
  return [
    `# 会话交接 · ${title}`,
    '',
    '## 当前目标',
    '[填写这次要完成的一件事]',
    '',
    '## 已确认的事实与结果',
    '[仅保留已验证结论，附文件或证据位置]',
    '',
    '## 下一步',
    '[写明下一项可执行动作及验收条件]',
    '',
    '## 未解决的问题与约束',
    '[保留不确定项；不要把猜测当成结论]',
    '',
    '这是待填写的交接模板。填写并核实后，再复制到新会话。',
  ].join('\n')
}
