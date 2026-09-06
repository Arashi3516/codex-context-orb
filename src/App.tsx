import { useEffect, useRef, useState } from 'react'
import type { CSSProperties, PointerEvent } from 'react'
import {
  ArrowDownLeft, ArrowLeft, ArrowUpRight, BellOff, Check, ChevronDown,
  ChevronRight, CircleHelp, Copy, ExternalLink, Folder, Github, Info, Layers,
  LockKeyhole, Magnet, Moon, MoreHorizontal, Pin, Plus, Settings2, ShieldCheck, Sun, X,
} from 'lucide-react'
import { createHandoffTemplate, createReviewPrompt, describeProbe, evaluateContext, resolvePinned, SIGNAL_LABELS } from './lib/context'
import type { ContextSnapshot } from './lib/context'
import { ITEM_LABELS, RESULT_LABELS, RULE_LABELS, isEvidenceReport, type EvidenceReport } from './lib/evidence'
import { demoReport, demoSessions, DEMO_PRIMARY_ID, type DemoScenario } from './lib/demo'
import { beginNativeMagneticDrag, endNativeMagneticDrag, getNativeMagnetState, isNative, readEvidenceHistory, readLocalSessions, setNativeMagnetPreferences, sizeOrbWindow } from './lib/native'
import { DEFAULT_MAGNET, WINDOW_MODE_LABELS, dockPreview, latestMagnetState, parseMagnetPreferences, type MagnetPreferences, type MagnetState, type Rect, type WindowMagnetMode } from './lib/magnet'
import { presentCapacity, presentCompactions, presentRisk } from './lib/presentation'
import ContextReadout from './ContextReadout'

type PanelView = 'overview' | 'sessions' | 'handoff' | 'settings' | 'evidence' | 'review' | 'ledger' | 'history'
const widgetSurface = isNative || new URLSearchParams(location.search).get('surface') === 'orb'
const scenarioLabels: Record<DemoScenario, string> = {
  passed: '所列检查通过', failed: '发现约束偏差', superseded: '旧方案已作废', unknown: '关键证据缺失',
}

function readPreference(key: string, fallback: string) {
  try { return localStorage.getItem(key) ?? fallback } catch { return fallback }
}
function savePreference(key: string, value: string) {
  try { localStorage.setItem(key, value) } catch { /* preferences are optional */ }
}

function OrbMark({ className = '' }: { className?: string }) {
  return <svg className={className} viewBox="0 0 40 40" fill="none" aria-hidden="true">
    <path d="M10 23.5C10 13 18 7.7 28 10.7M30 16.5C30 27 22 32.3 12 29.3" stroke="currentColor" strokeWidth="3.1" strokeLinecap="round" />
  </svg>
}

export default function App() {
  const [theme, setTheme] = useState(() => readPreference('orb:theme', 'light'))
  const [scenario, setScenario] = useState<DemoScenario>('failed')
  const [sessions, setSessions] = useState<ContextSnapshot[]>(() => isNative ? [] : demoSessions('failed'))
  const [pinnedId, setPinnedId] = useState<string | null>(() => isNative ? readPreference('orb:pinned', '') || null : DEMO_PRIMARY_ID)
  const [expanded, setExpanded] = useState(!widgetSurface)
  const [view, setView] = useState<PanelView>('overview')
  const [now, setNow] = useState(Date.now())
  const [snoozedSessions, setSnoozedSessions] = useState<Record<string, number>>({})
  const [toast, setToast] = useState('')
  const [handoff, setHandoff] = useState('')
  const [help, setHelp] = useState(false)
  const [backgroundCount, setBackgroundCount] = useState(0)
  const [connectionError, setConnectionError] = useState(false)
  const [history, setHistory] = useState<EvidenceReport[]>([])
  const [historyError, setHistoryError] = useState(false)
  const [historyLoading, setHistoryLoading] = useState(false)
  const [historicalReport, setHistoricalReport] = useState<EvidenceReport | null>(null)
  const [offset, setOffset] = useState({ x: 0, y: 0 })
  const [dynamicColor, setDynamicColor] = useState(() => readPreference('orb:dynamic-color', 'true') !== 'false')
  const [showCompactions, setShowCompactions] = useState(() => readPreference('orb:show-compactions', 'false') === 'true')
  const [magnetPreferences, setMagnetPreferences] = useState(() => parseMagnetPreferences(readPreference('orb:magnet', JSON.stringify(DEFAULT_MAGNET))))
  const [magnetState, updateMagnetState] = useState<MagnetState | null>(null)
  function setMagnetState(incoming: MagnetState) { updateMagnetState(current => latestMagnetState(current, incoming)) }
  const [magnetSaving, setMagnetSaving] = useState(false)
  const [magnetInitialized, setMagnetInitialized] = useState(!isNative)
  const magnetWriteBusy = useRef(isNative)
  const magnetInitialization = useRef<Promise<MagnetState> | null>(null)
  const nativeGestureBusy = useRef(false)
  const [previewSnap, setPreviewSnap] = useState<'screen' | 'window' | null>(null)
  const [previewLayout, setPreviewLayout] = useState<{ left: boolean; top: boolean; width: number; height: number } | null>(null)
  const [dragging, setDragging] = useState(false)
  const orbRef = useRef<HTMLButtonElement>(null)
  const dockRef = useRef<HTMLDivElement>(null)
  const panelRef = useRef<HTMLElement>(null)
  const stageRef = useRef<HTMLElement>(null)
  const helpRef = useRef<HTMLElement>(null)
  const helpTriggerRef = useRef<HTMLButtonElement>(null)
  const pointer = useRef<{ x: number; y: number; moved: boolean; rect: Rect; stage: Rect; nativeStart?: Promise<MagnetState> } | null>(null)
  const resizeQueue = useRef(Promise.resolve())
  const snapshot = resolvePinned(sessions, pinnedId)
  const health = evaluateContext(snapshot, now)
  const risk = presentRisk(snapshot, now)
  const capacity = presentCapacity(snapshot, now)
  const compactions = presentCompactions(snapshot)
  const snapped = isNative ? magnetState?.snappedX ?? magnetState?.snappedY : previewSnap
  const dockLeft = isNative ? magnetState?.layoutAnchorLeft : previewLayout?.left
  const dockTop = isNative ? magnetState?.layoutAnchorTop : previewLayout?.top
  const magnetControlsDisabled = isNative && (!magnetInitialized || magnetSaving)
  const assessment = snapshot?.assessment
  const report = health.report
  const displayedReport = historicalReport?.session_id === pinnedId ? historicalReport : report
  const displayedHealth = displayedReport && snapshot ? evaluateContext({ ...snapshot, report: displayedReport }, now) : health
  const reviewPrompt = createReviewPrompt(isNative ? pinnedId : null)
  const snoozed = !!pinnedId && now < (snoozedSessions[pinnedId] ?? 0)

  useEffect(() => {
    document.documentElement.dataset.theme = theme
    document.documentElement.classList.toggle('widget-surface', widgetSurface)
    document.body.classList.toggle('widget-surface', widgetSurface)
    savePreference('orb:theme', theme)
  }, [theme])
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 1000)
    return () => window.clearInterval(timer)
  }, [])
  useEffect(() => {
    if (!isNative) return
    let cancelled = false
    let pending = false
    const refresh = async () => {
      if (pending) return
      pending = true
      try {
        const fresh = await readLocalSessions(pinnedId)
        if (!cancelled) { setSessions(fresh); setConnectionError(false) }
      } catch {
        if (!cancelled) { setSessions([]); setConnectionError(true) }
      } finally { pending = false }
    }
    void refresh()
    const timer = window.setInterval(() => void refresh(), 3000)
    return () => { cancelled = true; window.clearInterval(timer) }
  }, [pinnedId])
  useEffect(() => {
    if (view !== 'history' || !pinnedId) return
    let cancelled = false
    setHistory([])
    setHistoryError(false)
    setHistoryLoading(true)
    const load = isNative ? readEvidenceHistory(pinnedId) : Promise.resolve(report ? [report,
      { ...demoReport('passed', report.reviewed_at_ms - 15 * 60_000), session_id: pinnedId, report_id: 'e'.repeat(64) }] : [])
    void load.then(items => {
      if (cancelled) return
      if (items.some(item => !isEvidenceReport(item) || item.session_id !== pinnedId)) throw new Error('Invalid history scope')
      setHistory(items)
    }).catch(() => { if (!cancelled) setHistoryError(true) }).finally(() => { if (!cancelled) setHistoryLoading(false) })
    return () => { cancelled = true }
  }, [view, pinnedId, report])
  useEffect(() => {
    if (!isNative) return
    resizeQueue.current = resizeQueue.current.then(async () => {
      const state = await sizeOrbWindow(expanded)
      if (state) setMagnetState(state)
    }).catch(() => {
      setToast('窗口尺寸调整失败，可拖动悬浮球重新定位。')
    })
  }, [expanded])
  useEffect(() => {
    if (!isNative) return
    let cancelled = false, pending = false
    let lastRead = 0
    const refresh = async () => {
      if (pending || Date.now() - lastRead < (pointer.current || expanded ? 120 : 800)) return
      pending = true; lastRead = Date.now()
      try { const state = await getNativeMagnetState(); if (!cancelled) setMagnetState(state) } catch { /* surfaced when the user requests window actions */ }
      finally { pending = false }
    }
    void refresh()
    const timer = window.setInterval(() => void refresh(), 120)
    return () => { cancelled = true; window.clearInterval(timer) }
  }, [expanded])
  useEffect(() => {
    if (!isNative) return
    let cancelled = false
    magnetInitialization.current ??= setNativeMagnetPreferences(magnetPreferences).catch(async () => {
      setToast('保存的磁吸设置未应用，已重新读取当前设置。')
      return getNativeMagnetState()
    })
    void magnetInitialization.current.then(state => {
      if (cancelled) return
      setMagnetState(state); setMagnetPreferences(state.preferences); setMagnetInitialized(true)
      magnetWriteBusy.current = false
    }).catch(() => { if (!cancelled) setToast('磁吸控制暂不可用，请确认原生应用已更新后重新打开。') })
    return () => { cancelled = true }
  }, [])
  useEffect(() => { savePreference('orb:dynamic-color', String(dynamicColor)) }, [dynamicColor])
  useEffect(() => { savePreference('orb:show-compactions', String(showCompactions)) }, [showCompactions])
  useEffect(() => { savePreference('orb:magnet', JSON.stringify(magnetPreferences)); setPreviewSnap(null) }, [magnetPreferences])
  useEffect(() => {
    if (!toast) return
    const timer = window.setTimeout(() => setToast(''), 4000)
    return () => window.clearTimeout(timer)
  }, [toast])
  useEffect(() => {
    if (expanded && panelRef.current) {
      panelRef.current.scrollTop = 0
      panelRef.current.focus({ preventScroll: true })
    }
  }, [expanded, view])
  useEffect(() => {
    if (!help) return
    helpRef.current?.querySelector<HTMLButtonElement>('button')?.focus({ preventScroll: true })
    return () => helpTriggerRef.current?.focus({ preventScroll: true })
  }, [help])

  function chooseScenario(next: DemoScenario) {
    setScenario(next)
    setSessions(demoSessions(next))
    setPinnedId(DEMO_PRIMARY_ID)
    setView(next === 'superseded' ? 'ledger' : 'overview')
    setHistoricalReport(null)
    setExpanded(true)
    setSnoozedSessions({})
    setNow(Date.now())
  }
  function closePanel() {
    setExpanded(false)
    setView('overview')
    orbRef.current?.focus({ preventScroll: true })
  }
  function pinSession(id: string) {
    setPinnedId(id)
    if (isNative) savePreference('orb:pinned', id)
    setView('overview')
    setHistoricalReport(null)
  }
  function prepareHandoff() {
    setHandoff(createHandoffTemplate(snapshot))
    setView('handoff')
  }
  async function copyText(value: string, notice: string) {
    try {
      await navigator.clipboard.writeText(value)
      setToast(notice)
    } catch {
      setToast('无法访问剪贴板，请在文本框内全选并复制。')
    }
  }
  function simulateClarification() {
    if (isNative) return
    setSessions(current => current.map(item => item.id !== pinnedId || !item.report ? item : {
      ...item, report: { ...demoReport('passed'), session_id: item.id, turn_id: item.report.turn_id },
    }))
    setScenario('passed')
    setHistoricalReport(null)
    setView('overview')
    setToast('已模拟修改文件并重新采集。所列检查通过，新开收益仍未评估。')
  }
  function snooze() {
    if (!pinnedId) return
    setSnoozedSessions(current => ({ ...current, [pinnedId]: Date.now() + 10 * 60 * 1000 }))
    setToast('此会话已稍后提醒，10 分钟内保持安静。')
    closePanel()
  }
  function backgroundActivity() {
    setSessions(current => current.map(item => item.id === DEMO_PRIMARY_ID ? item : { ...item, observedAt: Date.now() }))
    setBackgroundCount(count => count + 1)
    setToast('后台会话已更新，当前固定会话保持不变。')
  }
  async function changeMagnet(next: Partial<MagnetPreferences>) {
    if (magnetWriteBusy.current) return
    const preferences = { ...magnetPreferences, ...next }
    magnetWriteBusy.current = true; setMagnetSaving(true)
    try {
      if (isNative) {
        const state = await setNativeMagnetPreferences(preferences)
        setMagnetState(state)
        setMagnetPreferences(state.preferences)
      } else setMagnetPreferences(preferences)
    } catch { setToast('磁吸设置未应用，请重试。') }
    finally { magnetWriteBusy.current = false; setMagnetSaving(false) }
  }
  function pointerDown(event: PointerEvent<HTMLButtonElement>) {
    if (event.button !== 0) return
    if (isNative && nativeGestureBusy.current) return
    const stage = stageRef.current?.getBoundingClientRect(), ball = orbRef.current?.getBoundingClientRect()
    if (!stage || !ball) return
    pointer.current = { x: isNative ? event.screenX : event.clientX, y: isNative ? event.screenY : event.clientY, moved: false,
      rect: { x: ball.x - stage.x, y: ball.y - stage.y, width: ball.width, height: ball.height },
      stage: { x: stage.x, y: stage.y, width: stage.width, height: stage.height } }
    event.currentTarget.setPointerCapture(event.pointerId)
    if (isNative) {
      nativeGestureBusy.current = true
      pointer.current.nativeStart = beginNativeMagneticDrag(event.clientX / innerWidth, event.clientY / innerHeight)
      void pointer.current.nativeStart.catch(() => setToast('拖动暂不可用。'))
    }
  }
  function pointerMove(event: PointerEvent<HTMLButtonElement>) {
    const start = pointer.current
    if (!start) return
    const dx = (isNative ? event.screenX : event.clientX) - start.x
    const dy = (isNative ? event.screenY : event.clientY) - start.y
    if (!start.moved && Math.hypot(dx, dy) < 4) return
    start.moved = true
    setDragging(true)
    if (isNative) return
    const stage = start.stage
    const rect = { ...start.rect, x: Math.max(6, Math.min(stage.width - start.rect.width - 6, start.rect.x + dx)),
      y: Math.max(6, Math.min(stage.height - start.rect.height - 6, start.rect.y + dy)) }
    if (!previewLayout) setPreviewLayout(previewPanelLayout(rect, stage))
    setPreviewSnap(null)
    setOffset({ x: rect.x, y: rect.y })
  }
  function previewPanelLayout(rect: Rect, stage: { width: number; height: number }) {
    const left = stage.width - rect.x >= rect.x + rect.width
    const top = stage.height - rect.y - rect.height >= rect.y
    return { left, top, width: (left ? stage.width - rect.x : rect.x + rect.width) - 6,
      height: Math.max(80, (top ? stage.height - rect.y - rect.height : rect.y) - 18) }
  }
  function finishPreviewDrag(start: NonNullable<typeof pointer.current>, x: number, y: number) {
    const stage = stageRef.current?.getBoundingClientRect()
    if (!stage) return
    const mock = stageRef.current?.querySelector('.mock-window')?.getBoundingClientRect()
    const windows = mock ? [{ x: mock.x - stage.x, y: mock.y - stage.y, width: mock.width, height: mock.height }] : []
    const result = dockPreview({ ...start.rect, x: start.rect.x + x - start.x, y: start.rect.y + y - start.y },
      { x: 6, y: 6, width: stage.width - 12, height: stage.height - 12 }, windows, magnetPreferences, { x: x - stage.x, y: y - stage.y })
    setOffset({ x: result.rect.x, y: result.rect.y })
    setPreviewSnap(result.kind)
    setPreviewLayout(previewPanelLayout(result.rect, stage))
  }
  async function pointerUp(event: PointerEvent<HTMLButtonElement>) {
    if (event.button !== 0 || !pointer.current) return
    const start = pointer.current
    pointer.current = null
    setDragging(false)
    let wasMoved = start.moved || Math.hypot((isNative ? event.screenX : event.clientX) - start.x, (isNative ? event.screenY : event.clientY) - start.y) >= 4
    const release = { anchorX: event.clientX / innerWidth, anchorY: event.clientY / innerHeight, moved: wasMoved }
    if (isNative) {
      try {
        await start.nativeStart
        const state = await endNativeMagneticDrag(release)
        setMagnetState(state)
        wasMoved = wasMoved || state.lastDragMoved || Math.hypot(event.screenX - start.x, event.screenY - start.y) >= 4
      } catch { return }
      finally { nativeGestureBusy.current = false }
    }
    if (wasMoved) {
      if (isNative) setExpanded(false)
      if (!isNative) finishPreviewDrag(start, event.clientX, event.clientY)
      return
    }
    setExpanded(open => !open)
    setView('overview')
  }
  function pointerCancel() {
    const start = pointer.current
    pointer.current = null; setDragging(false)
    if (isNative && start) void Promise.resolve(start.nativeStart).then(() => endNativeMagneticDrag()).then(setMagnetState).catch(() => {}).finally(() => { nativeGestureBusy.current = false })
    else if (start?.moved) finishPreviewDrag(start, start.x + offset.x - start.rect.x, start.y + offset.y - start.rect.y)
  }
  async function keyboardToggle() {
    if (nativeGestureBusy.current) return
    if (isNative) {
      nativeGestureBusy.current = true
      try { setMagnetState(await getNativeMagnetState()) }
      catch { setToast('窗口状态暂不可用，请重试。'); return }
      finally { nativeGestureBusy.current = false }
    }
    setExpanded(open => !open)
    setView('overview')
  }

  const orb = <div ref={dockRef} className={`orb-dock level-${health.level} risk-${dynamicColor ? risk.tone : 'neutral'} ${previewLayout ? 'preview-placed' : ''} ${dockLeft ? 'dock-left' : ''} ${dockTop ? 'dock-top' : ''} ${dragging || magnetState?.dragging ? 'is-dragging' : ''} ${snapped ? 'is-snapped' : ''}`} style={{ '--orb-x': `${offset.x}px`, '--orb-y': `${offset.y}px`, '--preview-panel-width': `${previewLayout?.width ?? 340}px`, '--preview-panel-height': `${previewLayout?.height ?? 600}px` } as CSSProperties}>
    {expanded && <section
      id="orb-panel" className="orb-panel" data-testid="orb-panel" ref={panelRef} tabIndex={-1}
      role="dialog" aria-label="会话状态" onKeyDown={event => { if (event.key === 'Escape') closePanel() }}
    >
      <header className="panel-top">
        <div className="panel-brand"><OrbMark /><span>Context Orb</span><span className="version-pill">v0.4.3 预览</span></div>
        <button className="icon-button" aria-label="收起面板" onClick={closePanel}><X size={17} /></button>
      </header>
      {view !== 'overview' && <button className="back-button" onClick={() => { setView('overview'); setHistoricalReport(null) }}><ArrowLeft size={14} />返回概览</button>}
      {view === 'overview' && <>
        <button className="session-binding" onClick={() => setView('sessions')} aria-label="选择固定会话">
          <span className="binding-icon"><Pin size={14} /></span>
          <span className="binding-copy"><strong data-testid="bound-session">{snapshot?.title ?? '尚未固定会话'}</strong><small>{snapshot ? '手动固定 · 不随后台任务切换' : '点击选择要关注的会话'}</small></span>
          <ChevronDown size={15} />
        </button>
        <ContextReadout snapshot={snapshot} now={now} showCompactions={showCompactions} />
        <div className="semantic-card" data-testid="semantic-card">
          <div className="semantic-card-label"><span>下一步的检查依据</span><span className="source-label">{snapshot?.source === 'demo' ? '模拟收据' : report ? '本地文件检查' : '尚未收集'}</span></div>
          {report ? <>
            <div className="check-tally"><span><b>{health.checks.passed}</b> 通过</span><span><b>{health.checks.failed}</b> 偏差</span><span><b>{health.checks.unknown}</b> 未知</span></div>
            {report.probes.some(probe => probe.result !== 'pass') ? <div className="signal-previews">{report.probes.filter(probe => probe.result !== 'pass').slice(0, 2).map(probe => <div className="signal-preview" key={probe.id}><i /><span>{describeProbe(report, probe)}</span><small>{RESULT_LABELS[probe.result]}</small></div>)}</div>
              : !health.unresolved.length && <div className="clear-result"><ShieldCheck size={16} /><span>所列条件在采集的文件版本中通过</span></div>}
            {!!health.unresolved.length && <p className="overview-gap">待补齐：{health.unresolved[0]}</p>}
            <div className="overview-links"><button className="evidence-link" onClick={() => { setHistoricalReport(null); setView('evidence') }}>查看检查与来源<ArrowUpRight size={14} /></button>
            <button className="evidence-link" onClick={() => { setHistoricalReport(null); setView('ledger') }}>查看任务账本<ChevronRight size={14} /></button></div>
          </> : <div className="review-empty"><CircleHelp size={24} /><strong>等待一次有依据的检查</strong><p>声明有效要求，检查明确选定的文件。无法验证的前提继续标为未知。</p>{assessment && <button className="evidence-link" onClick={() => setView('evidence')}>回顾旧版评估<ArrowUpRight size={14} /></button>}</div>}
        </div>
        <div className="quiet-note"><Info size={16} /><p>{connectionError ? '本地报告暂不可读，请检查接入配置。' : report ? '截至本次采集 · 新开收益尚未评估' : '由你在目标会话发起收集，悬浮球展示本地结果。'}<br /><span>{health.notices[1] ?? (report ? '文件检查结果不代表整个上下文已被验证。' : '声明来源、实际检查与未知项分别保留。')}</span></p></div>
        <div className="panel-actions">
          <button className="primary-button" onClick={!report ? () => setView('review') : prepareHandoff}>
            {!report ? '准备证据检查' : '整理下一步简报'}<ArrowUpRight size={16} />
          </button>
          <button className="secondary-button" disabled={!snapshot} onClick={snooze}><BellOff size={14} />稍后提醒</button>
        </div>
        <footer className="panel-footer"><span><i />{health.reviewedAt !== null ? `${isNative ? '采集于' : '演示采集'} ${new Date(health.reviewedAt).toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' })}` : '等待本地报告'}</span><button onClick={() => setView('settings')} aria-label="提醒设置"><Settings2 size={14} />设置</button></footer>
      </>}
      {view === 'sessions' && <div className="subview">
        <h2>只关注，你选中的会话</h2><p className="subview-intro">手动固定一个会话。后台任务的更新不会自动切换它。</p>
        <div className="session-list">
          {sessions.map(item => <button className={`session-option ${pinnedId === item.id ? 'selected' : ''}`} key={item.id} title={item.id} aria-label={`${item.title} · ${item.id}`} onClick={() => pinSession(item.id)}>
            <span className="session-avatar"><Layers size={17} /></span><span><strong>{item.title}</strong><small>{item.source === 'demo' ? '演示会话' : item.report ? '已有文件检查收据' : item.assessment ? '仅有旧版评估' : '已收到生命周期事件'}</small></span>{pinnedId === item.id ? <Check size={17} /> : <ChevronRight size={15} />}
          </button>)}
          {sessions.length === 0 && <div className="empty-state"><Layers size={28} /><strong>等待第一份检查</strong><p>启用仓库插件后，在目标 Codex 会话中请求 context-health 检查。</p><button className="text-button" onClick={() => setView('review')}>准备检查指令</button></div>}
        </div>
        {pinnedId && <button className="text-button" onClick={() => { setPinnedId(null); if (isNative) savePreference('orb:pinned', ''); setView('overview') }}>解除固定</button>}
        <p className="micro-note"><Info size={13} />当前版本需要手动固定会话。</p>
      </div>}
      {view === 'evidence' && <div className="subview evidence-view">
        <span className="status-chip status-unknown"><i />{historicalReport ? '历史收据' : '截至采集时点'}</span>
        <h2>{displayedReport ? '每项结果，都有依据' : '旧版评估，仅供回顾'}</h2>
        <p className="subview-intro">{displayedReport ? '文字检查只验证声明的条件。原始要求由评估者整理，文件版本通过本地读取记录。' : '旧版主观评估不再产生健康或新开建议。请准备新的来源账本与文件检查。'}</p>
        {displayedReport ? <>
          <p className="review-note">采集于 {new Date(displayedReport.reviewed_at_ms).toLocaleString('zh-CN')}<br />版本 {displayedReport.report_id.slice(0, 12)}</p>
          <div className="review-goal"><small>这次要做什么</small><p>{displayedReport.scope.next_step}</p></div>
          <div className="evidence-list">{displayedReport.probes.map(probe => <article className={`evidence-item probe-${probe.result}`} key={probe.id}>
            <header><strong>{RULE_LABELS[probe.rule]}</strong><span>{RESULT_LABELS[probe.result]}</span></header>
            <p>{describeProbe(displayedReport, probe)}</p>
            {probe.expected && <code className="probe-expected">{probe.expected}</code>}
            <p className="probe-detail">{probe.detail}</p>
            <small className="source-ref">{displayedReport.sources.find(source => source.id === probe.source_id)?.ref ?? '未进行自动文件检查'}</small>
          </article>)}</div>
          {!!displayedHealth.unresolved.length && <div className="unresolved-card"><strong>仍需补齐</strong><ul>{displayedHealth.unresolved.map(item => <li key={item}>{item}</li>)}</ul></div>}
          <h3 className="evidence-section-title">来源与版本</h3>
          <div className="source-list">{displayedReport.sources.map(source => <article className="source-item" key={source.id}>
            <header><strong>{source.ref}</strong><span>{source.status === 'captured' ? '已采集文件' : source.status === 'attested' ? '评估者声明' : '不可读取'}</span></header>
            <p>{source.note}</p>{source.sha256 && <code title={source.sha256}>SHA-256 {source.sha256}</code>}
          </article>)}</div>
          {!!displayedReport.observations.length && <><h3 className="evidence-section-title">评估者记录 · 尚非独立验证</h3>{displayedReport.observations.map(item => {
            const atom = displayedReport.ledger.find(entry => entry.id === item.item_id)!
            return <div className="recorded-observation" key={item.id}><strong>{SIGNAL_LABELS[item.kind]}</strong><p>{item.summary}</p>
              <small>关联：{atom.text} · {atom.status === 'superseded' ? '已作废，不参与当前判断' : atom.status === 'hypothesis' ? '待验证假设' : '本次有效'}</small>
              <p><small>{item.status === 'resolved' ? '记录为已解决' : '记录为未解决'}{item.recurrence === 'after_correction' && ' · 记录为纠正后复发'}<br />来源：{item.source_ids.map(id => displayedReport.sources.find(source => source.id === id)?.ref ?? id).join('；')}</small></p></div>
          })}</>}
          <button className="text-button review-again" onClick={() => setView('ledger')}>查看此报告的账本<ChevronRight size={14} /></button>
          {!isNative && !historicalReport && health.checks.failed > 0 && <button className="secondary-button full-width" onClick={simulateClarification}>模拟修改并重新采集<Check size={15} /></button>}
          <button className="text-button review-again" onClick={() => setView('history')}>查看采集历史<ChevronRight size={14} /></button>
        </> : assessment && <>
          <p className="review-note">旧版评估于 {new Date(assessment.reviewed_at_ms).toLocaleString('zh-CN')}</p>
          <div className="review-goal"><small>当时的目标</small><p>{assessment.current_goal}</p></div>
          {assessment.signals.map(signal => <article className="evidence-item" key={signal.id}><strong>{SIGNAL_LABELS[signal.kind]}</strong><p>{signal.summary}</p></article>)}
        </>}
        <button className="text-button review-again" onClick={() => setView('review')}>准备重新收集<ChevronRight size={14} /></button>
      </div>}
      {view === 'ledger' && <div className="subview ledger-view">
        <span className="status-chip status-unknown"><i />{historicalReport ? '历史账本' : '声明的任务范围'}</span><h2>留下有效的，标明作废的</h2>
        <p className="subview-intro">每个条目保留来源和变更关系。这份账本由评估者整理，不代表插件已经独立读取全部原始要求。</p>
        {displayedReport?.ledger.map(item => <article className={`ledger-item ledger-${item.status}`} key={item.id}>
          <header><strong>{ITEM_LABELS[item.kind]}</strong><span>{item.status === 'superseded' ? '已作废' : item.status === 'hypothesis' ? '待验证假设' : '本次有效'}{item.critical && ' · 关键'}</span></header><p>{item.text}</p>
          <small>{item.source_ids.map(id => displayedReport.sources.find(source => source.id === id)?.ref ?? id).join('；')}</small>
          {!!item.supersedes.length && <p className="supersession-note">替代：{item.supersedes.map(id => displayedReport.ledger.find(previous => previous.id === id)?.text ?? id).join('；')}</p>}
        </article>)}
        <button className="text-button review-again" onClick={() => setView('evidence')}>查看检查与来源<ChevronRight size={14} /></button>
      </div>}
      {view === 'history' && <div className="subview history-view">
        <h2>每次采集，单独保留</h2><p className="subview-intro">最多保留最近 8 份收据。历史结果只对应当时的文件与声明范围。</p>
        {historyLoading && <p role="status">正在读取历史…</p>}{historyError && <p role="alert">历史暂不可读，请重新收集或检查本地目录。</p>}
        {!historyLoading && !historyError && history.length === 0 && <p>尚无历史收据。</p>}
        <div className="history-list">{history.map(item => <button className="history-item" key={item.report_id} onClick={() => { setHistoricalReport(item); setView('evidence') }}>
          <span><strong>{new Date(item.reviewed_at_ms).toLocaleString('zh-CN')}</strong><small>{item.probes.length} 项检查 · {item.report_id.slice(0, 12)}</small></span><ChevronRight size={16} />
        </button>)}</div>
      </div>}
      {view === 'review' && <div className="subview handoff-view">
        <span className="status-chip status-unknown"><i />由你发起</span><h2>准备有来源的检查</h2>
        <p className="subview-intro">在目标 Codex 会话中发送这段指令。需要先启用仓库提供的 context-health 插件。</p>
        <label className="sr-only" htmlFor="review-text">证据检查指令</label><textarea id="review-text" readOnly value={reviewPrompt} />
        <button className="primary-button full-width" onClick={() => void copyText(reviewPrompt, '已复制。请在目标 Codex 会话中发送；检查尚未运行。')}><Copy size={15} />复制检查指令</button>
        <p className="micro-note">保存声明的要求、文件版本和检查结果。收集器只读取明确选定的工作区文件，不读取私有转录或调用额外模型。</p>
      </div>}
      {view === 'handoff' && <div className="subview handoff-view">
        <span className="status-chip status-unknown"><i />新开收益尚未评估</span>
        <h2>让下一步有据可循</h2>
        <p className="subview-intro">这份简报也可用于原会话内纠正。先核对要求、未知项和文件版本，再决定在哪继续。</p>
        <label className="sr-only" htmlFor="handoff-text">下一步任务简报</label>
        <textarea id="handoff-text" value={handoff} onChange={event => setHandoff(event.target.value)} spellCheck={false} />
        <button className="primary-button full-width" onClick={() => void copyText(handoff, '已复制。可先在原会话核对并纠正；新开收益尚未评估。')}><Copy size={15} />复制任务简报</button>
        <p className="micro-note">检查收据只对应采集时点；目标或文件改变后需要重新核验。</p>
      </div>}
      {view === 'settings' && <div className="subview settings-view">
        <h2>按你的习惯停靠</h2><p className="subview-intro">拖动后松手，自动停靠最近的边框。</p>
        <fieldset className="magnet-settings"><legend><Magnet size={14} />窗口吸附</legend>
          <div className="window-modes">{(['codex', 'off', 'all'] as WindowMagnetMode[]).map(mode => <label key={mode} className={magnetPreferences.windowMode === mode ? 'selected' : ''}><input type="radio" name="window-magnet" aria-label={mode === 'codex' ? '仅限 Codex 窗口' : WINDOW_MODE_LABELS[mode]} disabled={magnetControlsDisabled} checked={magnetPreferences.windowMode === mode} onChange={() => void changeMagnet({ windowMode: mode })} /><span>{WINDOW_MODE_LABELS[mode]}</span></label>)}</div>
          <p>{magnetPreferences.windowMode === 'codex' ? '在 Codex 窗口内或球贴到边框时松手，平滑停靠并随窗口移动。' : magnetPreferences.windowMode === 'all' ? '在可见窗口内或球贴到边框时松手，平滑停靠并随窗口移动。' : '松手后，平滑停靠当前屏幕最近的边缘。'}{magnetPreferences.windowMode !== 'off' && '其余位置停靠屏幕边缘。'}</p>
          {isNative && magnetState?.capabilities.reason && <p className="magnet-caveat">窗口信息暂不可用，请确认桌面会话与显示器可用后重试。</p>}
          {isNative && magnetState?.capabilities.codexGui === 'unavailable' && magnetPreferences.windowMode === 'codex' && <p className="magnet-caveat">此平台暂不能可靠识别 Codex，可切换为「所有窗口」。</p>}
          {!isNative && <small>此处演示网页内拖拽；桌面版使用真实窗口边界。</small>}
        </fieldset>
        <div className="setting-row"><div><strong>球体动态配色</strong><small>随脏度线索变化，保留状态符号</small></div><button className="switch" role="switch" aria-label="球体动态配色" aria-checked={dynamicColor} onClick={() => setDynamicColor(value => !value)}><span /></button></div>
        <div className="setting-row"><div><strong>显示压缩次数</strong><small>在容量下方显示，不参与脏度判断</small></div><button className="switch" role="switch" aria-label="显示压缩次数" aria-checked={showCompactions} onClick={() => setShowCompactions(value => !value)}><span /></button></div>
        <div className="setting-row"><div><strong>外观</strong><small>与你的工作环境协调</small></div><button className="theme-toggle" onClick={() => setTheme(theme === 'light' ? 'dark' : 'light')} aria-label="切换面板主题">{theme === 'light' ? <Moon size={17} /> : <Sun size={17} />}</button></div>
        <div className="setting-row"><div><strong>稍后提醒</strong><small>{snoozed ? '此会话已暂停 10 分钟' : '需要安静时，暂停 10 分钟'}</small></div><button className="text-button" disabled={!snapshot} onClick={() => { if (snoozed && pinnedId) { setSnoozedSessions(current => ({ ...current, [pinnedId]: 0 })); setToast('此会话提醒已恢复。') } else snooze() }}>{snoozed ? '恢复' : '暂停'}</button></div>
        <div className="setting-row"><div><strong>检查依据</strong><small>声明的条件与采集的文件版本</small></div><button className="text-button" onClick={() => setView('review')}>复查</button></div>
        <div className="privacy-card"><LockKeyhole size={18} /><strong>保留依据，减少冗余</strong><p>本地保存任务条目、来源位置和检查收据，不上传对话。文件检查由你发起，未知信息继续标为未知。</p></div>
        <p className="micro-note">当前仅显示采集时点的结果。自动语义检测、系统通知与新开收益校准仍待验证。</p>
      </div>}
    </section>}
    <div className="orb-bottom-row">
      {!expanded && !widgetSurface && !previewLayout && <div className="resting-caption"><span>{snoozed ? '安静 10 分钟' : health.label}</span><ArrowDownLeft size={14} /></div>}
      <button
        ref={orbRef} className={`orb-button ${snoozed ? 'snoozed' : ''}`}
        aria-label={expanded ? '收起 Context Orb' : '展开 Context Orb'} aria-expanded={expanded}
        aria-controls="orb-panel" aria-describedby="orb-state-description" title={`${risk.value} · ${capacity.percent === null ? '用量未接入' : `容量 ${capacity.percent}%（演示）`} · 点击展开，拖动定位`}
        onPointerDown={pointerDown} onPointerMove={pointerMove} onPointerUp={event => void pointerUp(event)} onPointerCancel={pointerCancel} onLostPointerCapture={pointerCancel}
        onClick={event => { if (event.detail === 0) void keyboardToggle() }}
      >
        <svg className={`orb-ring ${capacity.ratio === null ? 'capacity-unknown' : 'capacity-known'}`} viewBox="0 0 80 80" aria-hidden="true">
          <circle className="orb-ring-base" cx="40" cy="40" r="36" />
          <circle className="orb-ring-fill" cx="40" cy="40" r="36" pathLength="100" strokeDasharray={capacity.ratio === null ? '1 7' : '100 100'} strokeDashoffset={capacity.ratio === null ? 0 : 100 - capacity.ratio * 100} />
        </svg>
        <span className="orb-core">{snoozed ? <BellOff size={22} /> : capacity.percent !== null ? <span className="orb-capacity">{capacity.percent}<small>%</small></span> : <OrbMark />}</span>
        <span className="orb-status-dot" aria-hidden="true">{risk.tone === 'aligned' ? '✓' : risk.tone === 'deviation' ? '!' : '?'}</span>
        {(dragging || magnetState?.dragging) && <span className="snap-indicator" aria-hidden="true"><Magnet size={11} /></span>}
      </button>
      <span id="orb-state-description" className="sr-only">脏度线索：{risk.value}。上下文容量：{capacity.percent === null ? '未接入' : `${capacity.percent}%，演示用量`}。{showCompactions && `压缩次数：${compactions.value}。`}{snapped && `已吸附${snapped === 'screen' ? '屏幕' : '窗口'}边缘。`}</span>
    </div>
  </div>

  if (widgetSurface) return <main className="native-stage" ref={stageRef}>{orb}{toast && expanded && <div role="status" className="toast native-toast">{toast}</div>}</main>

  return <div className="studio-shell">
    <header className="studio-header">
      <a className="wordmark" href="/" aria-label="Context Orb 首页"><span className="brand-symbol"><OrbMark /></span><strong>context<span>orb</span></strong></a>
      <nav><span className="preview-badge"><i />交互设计预览</span><button className="icon-button" aria-label="切换主题" onClick={() => setTheme(theme === 'light' ? 'dark' : 'light')}>{theme === 'light' ? <Moon size={19} /> : <Sun size={19} />}</button><button ref={helpTriggerRef} className="icon-button" aria-label="查看设计说明" onClick={() => setHelp(true)}><CircleHelp size={19} /></button></nav>
    </header>
    <main className="studio-main">
      <section className="intro">
        <div className="eyebrow"><span />EVIDENCE FOR YOUR NEXT STEP</div>
        <h1>专注，<br />让思路<span className="serif-word">清楚。</span></h1>
        <p className="intro-copy">留下有效的要求，核对这一步的依据。<br />让旧说法有去处，让下一步看得清。</p>
        <div className="platform-row"><span><svg viewBox="0 0 20 20" aria-hidden="true"><path d="M13.4 3.7c.8-1 1-2 .9-2.7-1 .1-2.1.6-2.8 1.5-.7.8-1 1.9-.9 2.7 1 .1 2-.5 2.8-1.5ZM16.9 14.2c-.4.9-.6 1.3-1.1 2.1-.7 1-1.6 2.4-2.8 2.4-1.1 0-1.4-.7-2.9-.7s-1.8.7-2.9.7c-1.2 0-2-1.2-2.7-2.2C2.5 13.6 2 9.7 3.3 7.7c.9-1.4 2.3-2.1 3.6-2.1 1.2 0 2 .7 3 .7s1.6-.7 3-.7c1.1 0 2.4.6 3.3 1.7-2.9 1.6-2.4 5.6.7 6.9Z" fill="currentColor"/></svg>macOS</span><span><svg viewBox="0 0 20 20" aria-hidden="true"><path d="m2 4 7-1v6H2V4Zm8-1.2L18 2v7h-8V2.8ZM2 10h7v6l-7-1v-5Zm8 0h8v7l-8-1v-6Z" fill="currentColor"/></svg>Windows</span><span className="platform-stage">框架 v0.4</span></div>
        <div className="scenario-controls">
          <div className="section-label"><span>试试四种状态</span><span>01 — 04</span></div>
          <div className="scenario-grid" role="group" aria-label="演示状态">
            {(Object.keys(scenarioLabels) as DemoScenario[]).map((level, index) => <button key={level} className={`scenario-button level-${level === 'failed' ? 'watch' : level === 'unknown' ? 'unknown' : 'healthy'} ${scenario === level ? 'active' : ''}`} aria-pressed={scenario === level} onClick={() => chooseScenario(level)}><span className="scenario-indicator" /><span>{scenarioLabels[level]}</span><small>0{index + 1}</small></button>)}
          </div>
        </div>
        <div className="design-principle"><span className="principle-line" /><p>声明、检查、未知，分别保留。<br /><span>先核对，再决定怎样继续。</span></p></div>
      </section>
      <section className="workspace-stage" ref={stageRef} aria-label="悬浮球交互预览">
        <div className="stage-caption"><span className="stage-live-dot" />你的工作空间<span>示意场景 · 非真实会话</span></div>
        <div className="mock-window" aria-hidden="true">
          <div className="mock-window-bar"><div className="traffic-lights"><i /><i /><i /></div><span>studio / atlas</span><MoreHorizontal size={16} /></div>
          <div className="mock-window-content"><aside className="mock-sidebar"><div className="mock-project"><Folder size={15} />atlas</div><div className="mock-new"><Plus size={14} />新会话</div><small>进行中</small><div className="mock-selected">设置页交互优化</div><div>设置页验证（后台）</div><small>工作空间</small><div className="mock-file"><Folder size={13} />src</div><div className="mock-file nested">components</div><div className="mock-file nested">settings.tsx</div></aside><div className="mock-conversation"><div className="mock-thread-title">设置页交互优化<span>本地</span></div><div className="mock-user">把设置页的交互再收敛一下。</div><div className="mock-assistant"><span className="tiny-orb"><OrbMark /></span><div><strong>先让主操作更容易被找到。</strong><p>统一设置项的层级，保留清晰的反馈。<br />完成当前步骤后，再检查键盘导航。</p><div className="mock-code"><span><i>01</i><b>const</b> preferences = &#123;</span><span><i>02</i>&nbsp; theme: <em>'auto'</em>,</span><span><i>03</i>&nbsp; notifications: <em>'quiet'</em></span><span><i>04</i>&#125;</span></div><div className="mock-done"><Check size={13} />已整理当前交互</div></div></div><div className="mock-composer">继续这段工作…<span>↵</span></div></div></div>
        </div>
        <div className="ambient-note"><span className="ambient-rule" /><span>需要时，<br />它就在这里。</span></div>
        {orb}
        <div className="stage-bottom"><button onClick={backgroundActivity}><Layers size={14} />模拟后台会话更新{backgroundCount > 0 && <span>{backgroundCount}</span>}</button><span><Pin size={12} />始终绑定你的选择</span></div>
      </section>
    </main>
    <footer className="studio-footer"><span><LockKeyhole size={13} />本地优先 · 不上传对话</span><span>独立项目，与 OpenAI 无隶属关系</span><a href="https://github.com/Arashi3516/codex-context-orb" target="_blank" rel="noreferrer" className="repo-link"><Github size={15} />GitHub<ExternalLink size={11} /></a></footer>
    {toast && <div role="status" className="toast"><Check size={15} />{toast}</div>}
    {help && <div className="modal-scrim" onClick={() => setHelp(false)}><section ref={helpRef} className="help-modal" role="dialog" aria-modal="true" aria-labelledby="help-title" onClick={event => event.stopPropagation()} onKeyDown={event => {
      if (event.key === 'Escape') setHelp(false)
      if (event.key === 'Tab') {
        const buttons = helpRef.current?.querySelectorAll('button')
        if (!buttons?.length) return
        const first = buttons[0], last = buttons[buttons.length - 1]
        if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last.focus() }
        else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus() }
      }
    }}><button className="icon-button modal-close" onClick={() => setHelp(false)} aria-label="关闭设计说明"><X size={18} /></button><OrbMark /><h2 id="help-title">让下一步，重新清楚。</h2><p>这里演示有来源的任务账本和文件检查。所有会话与收据均为虚构，可查看检查、作废关系和历史版本。</p><ul><li><strong>来源分开：</strong>声明的要求、实际读取的文件与未知项各自保留。</li><li><strong>范围明确：</strong>结果只反映采集时点，文件和目标变化后需要复查。</li><li><strong>保留选择权：</strong>同一份简报可留在原会话使用，新开收益尚未评估。</li></ul><button className="primary-button full-width" onClick={() => setHelp(false)}>开始体验<ChevronRight size={16} /></button></section></div>}
  </div>
}
