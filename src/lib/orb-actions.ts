import type { ContextSnapshot } from './context'
import { presentCapacity, presentRisk } from './presentation'

export type OrbActionKind = 'select-session' | 'view-status' | 'view-evidence' | 'prepare-review' | 'compact-guide'
export interface OrbAction {
  kind: OrbActionKind
  label: string
  reason: string
  targetId: string | null
  basis: string
}

/** The current reader has no connection to the selected Desktop task's controller.
 * Only name actions that this application can actually perform. A file FAIL never
 * authorizes a new task, and demo occupancy never enables native operations. */
export function primaryOrbAction(snapshot: ContextSnapshot | undefined, now = Date.now()): OrbAction {
  const risk = presentRisk(snapshot, now)
  const capacity = presentCapacity(snapshot, now)
  const basis = JSON.stringify([snapshot?.id, snapshot?.source, risk.health.report?.report_id,
    risk.tone, risk.health.level, capacity.pressure, capacity.stale])
  const action = (kind: OrbActionKind, label: string, reason: string): OrbAction => ({
    kind, label, reason, targetId: snapshot?.id ?? null, basis,
  })
  if (!snapshot) return action('select-session', '选择会话', '固定一个会话，窗口吸附只改变位置。')
  if (!risk.health.report) return action('prepare-review', '准备检查指令', '在固定会话发起检查，保留当前目标与依据。')
  if (risk.health.checks.failed) {
    return action('view-evidence', '查看偏差依据', '先核对检查与来源，文件偏差不等于需要换会话。')
  }
  if (risk.observations) return action('view-evidence', '查看待核验线索', '先查看记录与来源，核验尚未确认的疑点。')
  if (risk.health.level !== 'healthy') return action('prepare-review', '准备补充检查', '尚有未核验前提，先补齐检查依据。')
  if (capacity.pressure === 'high' && !capacity.stale) {
    return action('compact-guide', '查看压缩指引', '容量紧张；尚未连接此会话的压缩操作。')
  }
  return action('view-status', '查看会话状态', '所列依据一致，可继续当前工作。')
}

export type OrbIntent = Pick<OrbAction, 'kind' | 'targetId' | 'basis'>

export function captureOrbIntent(action: OrbAction): OrbIntent {
  return { kind: action.kind, targetId: action.targetId, basis: action.basis }
}

export function isCurrentOrbIntent(intent: OrbIntent, action: OrbAction) {
  return intent.kind === action.kind && intent.targetId === action.targetId && intent.basis === action.basis
}
