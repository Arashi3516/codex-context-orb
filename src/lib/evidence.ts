export type ItemKind = 'goal' | 'constraint' | 'fact' | 'decision' | 'progress'
export type SignalKind = 'goal_drift' | 'constraint_loss' | 'decision_conflict' | 'stale_fact' | 'repeated_work'

export interface EvidenceSource {
  id: string
  kind: 'statement' | 'artifact'
  ref: string
  note: string
  status: 'attested' | 'captured' | 'unavailable'
  sha256: string | null
}
export interface LedgerItem {
  id: string
  kind: ItemKind
  text: string
  source_ids: string[]
  status: 'active' | 'superseded' | 'hypothesis'
  supersedes: string[]
  critical: boolean
}
export interface ProbeResult {
  id: string
  item_id: string
  source_id: string | null
  rule: 'contains' | 'not_contains' | 'sha256' | 'manual'
  expected: string
  result: 'pass' | 'fail' | 'unknown'
  detail: string
}
export interface Observation {
  id: string
  item_id: string
  kind: SignalKind
  summary: string
  status: 'open' | 'resolved'
  recurrence: 'once' | 'after_correction'
  source_ids: string[]
}
export interface EvidenceReport {
  schema_version: 2
  source: 'codex-evidence-review'
  report_id: string
  session_id: string
  turn_id: string | null
  reviewed_at_ms: number
  scope: {
    mode: 'as_of'
    origin: 'main' | 'unknown'
    action_id: string
    goal_id: string
    next_step: string
    coverage: 'declared' | 'partial'
    unknowns: string[]
  }
  sources: EvidenceSource[]
  ledger: LedgerItem[]
  probes: ProbeResult[]
  observations: Observation[]
}

export const ITEM_LABELS: Record<ItemKind, string> = {
  goal: '当前目标', constraint: '有效约束', fact: '事实', decision: '决策', progress: '进展',
}
export const RULE_LABELS: Record<ProbeResult['rule'], string> = {
  contains: '包含指定文本', not_contains: '不含指定文本', sha256: '文件版本一致', manual: '仍需人工核验',
}
export const RESULT_LABELS: Record<ProbeResult['result'], string> = {
  pass: '通过', fail: '未通过', unknown: '待核验',
}

const id = (value: unknown): value is string => typeof value === 'string' && /^[A-Za-z0-9][A-Za-z0-9_-]{0,127}$/.test(value)
const digest = (value: unknown): value is string => typeof value === 'string' && /^[a-f0-9]{64}$/.test(value)
const text = (value: unknown, min: number, max: number): value is string => typeof value === 'string'
  && [...value].length >= min && [...value].length <= max && !/[\u0000-\u001f\u007f-\u009f\ud800-\udfff]/u.test(value)
const member = (value: unknown, values: string[]) => typeof value === 'string' && values.includes(value)
const object = (value: unknown, keys: string): value is Record<string, unknown> => value !== null && typeof value === 'object'
  && !Array.isArray(value) && Object.keys(value).sort().join(' ') === keys.split(' ').sort().join(' ')
const array = (value: unknown, max: number): value is unknown[] => Array.isArray(value) && value.length <= max
function refs(value: unknown, minimum: number, maximum: number): value is string[] {
  return array(value, maximum) && value.length >= minimum && value.every(id) && new Set(value).size === value.length
}
function artifactRef(value: string): boolean {
  return !/[\\:]/.test(value) && value.split('/').every(part => {
    const stem = part.split('.')[0].toUpperCase()
    return !['', '.', '..'].includes(part) && !/[ .]$/.test(part)
      && !['CON', 'PRN', 'AUX', 'NUL', 'CONIN$', 'CONOUT$'].includes(stem) && !/^(COM|LPT)[1-9]$/.test(stem)
  })
}
function prohibitedRef(value: string): boolean {
  return value.split('/').some(part => /^(\.codex|\.env(?:\..*)?)$/i.test(part))
}

/** Rust verifies the stored snapshot hash. This guard also protects the rendering boundary. */
export function isEvidenceReport(value: unknown): value is EvidenceReport {
  if (!object(value, 'schema_version source report_id session_id turn_id reviewed_at_ms scope sources ledger probes observations')) return false
  if (value.schema_version !== 2 || value.source !== 'codex-evidence-review' || !digest(value.report_id) || !id(value.session_id)
    || (value.turn_id !== null && !id(value.turn_id)) || !Number.isSafeInteger(value.reviewed_at_ms) || Number(value.reviewed_at_ms) < 0) return false
  const scope = value.scope
  if (!object(scope, 'mode origin action_id goal_id next_step coverage unknowns') || scope.mode !== 'as_of'
    || !member(scope.origin, ['main', 'unknown']) || !id(scope.action_id) || !id(scope.goal_id)
    || !text(scope.next_step, 1, 500) || !member(scope.coverage, ['declared', 'partial'])
    || !array(scope.unknowns, 8) || !scope.unknowns.every(item => text(item, 1, 240))) return false
  if (!array(value.sources, 16) || !array(value.ledger, 24) || !array(value.probes, 32) || !array(value.observations, 8)) return false
  const sources = new Map<string, EvidenceSource>()
  for (const source of value.sources) {
    if (!object(source, 'id kind ref note status sha256') || !id(source.id) || sources.has(source.id)
      || !member(source.kind, ['statement', 'artifact']) || !text(source.ref, 1, 240) || !text(source.note, 1, 240)) return false
    if (source.kind === 'statement' && (source.status !== 'attested' || source.sha256 !== null)) return false
    if (source.kind === 'artifact' && !((source.status === 'captured' && digest(source.sha256))
      || (source.status === 'unavailable' && source.sha256 === null))) return false
    if (source.kind === 'artifact' && (!artifactRef(source.ref)
      || (source.status === 'captured' && prohibitedRef(source.ref)))) return false
    sources.set(source.id, source as unknown as EvidenceSource)
  }
  const items = new Map<string, LedgerItem>()
  for (const item of value.ledger) {
    if (!object(item, 'id kind text source_ids status supersedes critical') || !id(item.id) || items.has(item.id)
      || !member(item.kind, Object.keys(ITEM_LABELS)) || !text(item.text, 1, 500)
      || !member(item.status, ['active', 'superseded', 'hypothesis']) || typeof item.critical !== 'boolean'
      || !refs(item.source_ids, 1, 4) || !item.source_ids.every(ref => sources.has(ref)) || !refs(item.supersedes, 0, 4)) return false
    items.set(item.id, item as unknown as LedgerItem)
  }
  if (items.get(scope.goal_id)?.kind !== 'goal' || items.get(scope.goal_id)?.status !== 'active') return false
  const visiting = new Set<string>(), visited = new Set<string>()
  function acyclic(item: LedgerItem): boolean {
    if (visiting.has(item.id)) return false
    if (visited.has(item.id)) return true
    visiting.add(item.id)
    for (const ref of item.supersedes) {
      const previous = items.get(ref)
      if (!previous || previous.status !== 'superseded' || !acyclic(previous)) return false
    }
    visiting.delete(item.id)
    visited.add(item.id)
    return true
  }
  if (![...items.values()].every(acyclic)) return false
  const probeIds = new Set<string>()
  for (const probe of value.probes) {
    if (!object(probe, 'id item_id source_id rule expected result detail') || !id(probe.id) || probeIds.has(probe.id)
      || !id(probe.item_id) || items.get(probe.item_id)?.status !== 'active'
      || !member(probe.rule, Object.keys(RULE_LABELS)) || !member(probe.result, Object.keys(RESULT_LABELS))
      || !text(probe.detail, 1, 240)) return false
    if (probe.rule === 'manual') {
      if (probe.expected !== '' || probe.source_id !== null || probe.result !== 'unknown') return false
    } else {
      if (!id(probe.source_id) || sources.get(probe.source_id)?.kind !== 'artifact') return false
      if (probe.rule === 'sha256' ? !digest(probe.expected) : !text(probe.expected, 1, 240)) return false
      const source = sources.get(probe.source_id)!
      if (source.status === 'unavailable' && probe.result !== 'unknown') return false
      if (source.status === 'captured' && probe.result === 'unknown') return false
      if (probe.rule === 'sha256' && source.status === 'captured'
        && probe.result !== (source.sha256 === probe.expected ? 'pass' : 'fail')) return false
    }
    probeIds.add(probe.id)
  }
  const observations = new Set<string>()
  for (const observation of value.observations) {
    if (!object(observation, 'id item_id kind summary status recurrence source_ids') || !id(observation.id) || observations.has(observation.id)
      || !id(observation.item_id) || !items.has(observation.item_id)
      || !member(observation.kind, ['goal_drift', 'constraint_loss', 'decision_conflict', 'stale_fact', 'repeated_work'])
      || !text(observation.summary, 1, 240) || !member(observation.status, ['open', 'resolved'])
      || !member(observation.recurrence, ['once', 'after_correction']) || !refs(observation.source_ids, 1, 4)
      || !observation.source_ids.every(ref => sources.has(ref))) return false
    observations.add(observation.id)
  }
  return true
}
