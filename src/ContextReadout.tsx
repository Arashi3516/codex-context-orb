import { CircleHelp, Gauge, Layers3 } from 'lucide-react'
import type { ContextSnapshot } from './lib/context'
import { formatTokens, presentCapacity, presentCompactions, presentRisk } from './lib/presentation'

export default function ContextReadout({ snapshot, now, showCompactions }: {
  snapshot: ContextSnapshot | undefined; now: number; showCompactions: boolean
}) {
  const risk = presentRisk(snapshot, now)
  const capacity = presentCapacity(snapshot, now)
  const compactions = presentCompactions(snapshot)
  return <section className="context-readout" aria-label="上下文状态">
    <div className="risk-caption"><span>脏度线索</span><span>{risk.health.report ? '截至采集时点' : '等待有来源的检查'}</span></div>
    <div className="risk-value"><span className={`risk-symbol risk-${risk.tone}`} aria-hidden="true">{risk.tone === 'aligned' ? '✓' : risk.tone === 'deviation' ? '!' : risk.tone === 'review' ? '?' : '·'}</span><h2>{risk.value}</h2></div>
    <p className="risk-description">{risk.detail}</p>
    <div className="risk-legend" aria-label={`当前脏度线索：${risk.value}`}>
      {([['unknown', '未评估'], ['aligned', '依据一致'], ['review', '待核验'], ['deviation', '有偏差']] as const).map(([tone, label]) => <span key={tone} className={`risk-key risk-${tone} ${risk.tone === tone ? 'current' : ''}`}><i />{label}</span>)}
    </div>
    <details className="metric-explanation"><summary><CircleHelp size={12} />这些颜色代表什么</summary><p>颜色表达所列依据中的风险线索，不是整个上下文的污染率。文件检查失败也不等于模型已遗忘要求。容量和压缩次数独立展示，不改变脏度线索。</p></details>
    <div className={`capacity-readout capacity-${capacity.pressure}`} data-testid="capacity-readout">
      <div className="metric-heading"><span><Gauge size={14} />上下文容量</span><strong>{capacity.percent === null ? '未接入' : <>{capacity.percent}<small>%</small></>}</strong></div>
      {capacity.ratio === null ? <div className="capacity-track capacity-unavailable" aria-label="上下文使用量未知" />
        : <div className="capacity-track" role="progressbar" aria-label="上下文使用量" aria-valuemin={0} aria-valuemax={100} aria-valuenow={capacity.percent!}><span style={{ transform: `scaleX(${capacity.ratio})` }} /></div>}
      <div className="metric-detail"><span>{capacity.ratio === null ? '尚无可核验的 token 读数' : `${formatTokens(capacity.used)} / ${formatTokens(capacity.total)} tokens`}</span><span>{capacity.stale ? '历史演示读数' : capacity.source}</span></div>
    </div>
    {showCompactions && <div className="compaction-readout" data-testid="compaction-readout"><span><Layers3 size={13} />压缩次数</span><strong>{compactions.value}</strong><small>{compactions.detail}</small></div>}
  </section>
}
