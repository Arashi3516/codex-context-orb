import { useEffect, useRef, useState } from 'react'
import type { CSSProperties, PointerEvent } from 'react'
import {
  ArrowDownLeft, ArrowLeft, ArrowUpRight, BellOff, Check, ChevronDown,
  ChevronRight, CircleHelp, Copy, ExternalLink, Folder, Github, Info, Layers,
  LockKeyhole, Moon, MoreHorizontal, Pin, Plus, Settings2, ShieldCheck, Sun, X,
} from 'lucide-react'
import { createHandoffTemplate, createReviewPrompt, evaluateContext, resolvePinned, SIGNAL_LABELS } from './lib/context'
import type { ContextSnapshot, HealthLevel } from './lib/context'
import { demoSessions, DEMO_PRIMARY_ID } from './lib/demo'
import { dragNativeWindow, isNative, readLocalSessions, sizeOrbWindow } from './lib/native'

type PanelView = 'overview' | 'sessions' | 'handoff' | 'settings' | 'evidence' | 'review'
const widgetSurface = isNative || new URLSearchParams(location.search).get('surface') === 'orb'
const scenarioLabels: Record<HealthLevel, string> = {
  healthy: '压缩后清晰', watch: '局部混杂', handoff: '建议新开', unknown: '证据不足',
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
  const [scenario, setScenario] = useState<HealthLevel>('handoff')
  const [sessions, setSessions] = useState<ContextSnapshot[]>(() => isNative ? [] : demoSessions('handoff'))
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
  const [offset, setOffset] = useState({ x: 0, y: 0 })
  const orbRef = useRef<HTMLButtonElement>(null)
  const panelRef = useRef<HTMLElement>(null)
  const stageRef = useRef<HTMLElement>(null)
  const helpRef = useRef<HTMLElement>(null)
  const helpTriggerRef = useRef<HTMLButtonElement>(null)
  const pointer = useRef<{ x: number; y: number; originX: number; originY: number; moved: boolean; dragging: boolean } | null>(null)
  const resizeQueue = useRef(Promise.resolve())
  const snapshot = resolvePinned(sessions, pinnedId)
  const health = evaluateContext(snapshot, now)
  const assessment = snapshot?.assessment
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
    if (!isNative) return
    resizeQueue.current = resizeQueue.current.then(() => sizeOrbWindow(expanded)).catch(() => {
      setToast('窗口尺寸调整失败，可拖动悬浮球重新定位。')
    })
  }, [expanded])
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

  function chooseScenario(next: HealthLevel) {
    setScenario(next)
    setSessions(demoSessions(next))
    setPinnedId(DEMO_PRIMARY_ID)
    setView('overview')
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
    setSessions(current => current.map(item => item.id !== pinnedId || !item.assessment ? item : {
      ...item, assessment: { ...item.assessment, reviewed_at_ms: Date.now(),
        signals: item.assessment.signals.map(signal => ({ ...signal, status: 'resolved' })),
        review_note: '演示复查：已重新确认主动保存约束，最近修改已恢复一致；旧冲突不再计入提醒。',
      },
    }))
    setScenario('healthy')
    setView('overview')
    setToast('已模拟澄清与复查。已解决的问题不再触发建议。')
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
  function pointerDown(event: PointerEvent<HTMLButtonElement>) {
    if (event.button !== 0) return
    pointer.current = { x: event.clientX, y: event.clientY, originX: offset.x, originY: offset.y, moved: false, dragging: false }
    if (!isNative) event.currentTarget.setPointerCapture(event.pointerId)
  }
  function pointerMove(event: PointerEvent<HTMLButtonElement>) {
    const start = pointer.current
    if (!start) return
    const dx = event.clientX - start.x
    const dy = event.clientY - start.y
    if (!start.moved && Math.hypot(dx, dy) < 6) return
    start.moved = true
    if (isNative) {
      if (start.dragging) return
      start.dragging = true
      void dragNativeWindow().catch(() => setToast('拖动暂不可用。'))
      return
    }
    const stage = stageRef.current?.getBoundingClientRect()
    if (!stage) return
    const minX = -(stage.width - (expanded ? 378 : 108))
    const minY = -(stage.height - (expanded ? 620 : 120))
    setOffset({ x: Math.max(Math.min(0, minX), Math.min(0, start.originX + dx)), y: Math.max(Math.min(0, minY), Math.min(0, start.originY + dy)) })
  }
  function pointerUp(event: PointerEvent<HTMLButtonElement>) {
    if (event.button !== 0 || !pointer.current) return
    const wasMoved = pointer.current.moved
    if (wasMoved) event.preventDefault()
    pointer.current = null
    if (wasMoved) return
    setExpanded(open => !open)
    setView('overview')
  }

  const orb = <div className={`orb-dock level-${health.level}`} style={{ '--orb-x': `${offset.x}px`, '--orb-y': `${offset.y}px` } as CSSProperties}>
    {expanded && <section
      id="orb-panel" className="orb-panel" data-testid="orb-panel" ref={panelRef} tabIndex={-1}
      role="dialog" aria-label="会话状态" onKeyDown={event => { if (event.key === 'Escape') closePanel() }}
    >
      <header className="panel-top">
        <div className="panel-brand"><OrbMark /><span>Context Orb</span><span className="version-pill">v0.2 预览</span></div>
        <button className="icon-button" aria-label="收起面板" onClick={closePanel}><X size={17} /></button>
      </header>
      {view !== 'overview' && <button className="back-button" onClick={() => setView('overview')}><ArrowLeft size={14} />返回概览</button>}
      {view === 'overview' && <>
        <button className="session-binding" onClick={() => setView('sessions')} aria-label="选择固定会话">
          <span className="binding-icon"><Pin size={14} /></span>
          <span className="binding-copy"><strong data-testid="bound-session">{snapshot?.title ?? '尚未固定会话'}</strong><small>{snapshot ? '手动固定 · 不随后台任务切换' : '点击选择要关注的会话'}</small></span>
          <ChevronDown size={15} />
        </button>
        <div className="health-heading">
          <span className={`status-chip status-${health.level}`}><i />{health.label}</span>
          <h2>{health.headline}</h2>
          <p>{health.description}</p>
        </div>
        <div className="semantic-card" data-testid="semantic-card">
          <div className="semantic-card-label"><span>压缩后的执行脉络</span><span className="source-label">{snapshot?.source === 'demo' ? '模拟评估' : assessment ? 'Codex 手动评估' : '尚未评估'}</span></div>
          {health.level !== 'unknown' && assessment ? <>
            <div className="semantic-facts"><div><strong>{assessment.compactions_observed ?? '—'}<span> 次</span></strong><small>已观察的压缩</small></div><span className="fact-separator" /><div><strong>{health.actionable.length}<span> 项</span></strong><small>影响下一步的疑点</small></div></div>
            {health.actionable.length ? <div className="signal-previews">{health.actionable.slice(0, 2).map(signal => <div className="signal-preview" key={signal.id}><i /><span>{SIGNAL_LABELS[signal.kind]}</span><small>{signal.recurrence === 'after_correction' ? '纠正后再次出现' : '仍待核对'}</small></div>)}</div>
              : <div className="clear-result"><ShieldCheck size={16} /><span>当前目标、约束与有效结论保持一致</span></div>}
            <button className="evidence-link" onClick={() => setView('evidence')}>查看评估依据<ArrowUpRight size={14} /></button>
          </> : <div className="review-empty"><CircleHelp size={24} /><strong>等待一次有依据的检查</strong><p>核对当前目标、有效约束，以及压缩后的执行是否仍然一致。</p>{assessment && <button className="evidence-link" onClick={() => setView('evidence')}>回顾上次评估依据<ArrowUpRight size={14} /></button>}</div>}
        </div>
        {health.level === 'unknown' ? <div className="quiet-note"><Info size={16} /><p>{connectionError ? '本地评估暂不可读，请检查接入配置。' : '首版由你在目标 Codex 会话中发起评估；悬浮球读取本地结果。'}</p></div>
          : <div className="quiet-note"><ShieldCheck size={16} /><p>{health.level === 'handoff' ? '先提取仍有效的信息，再继续下一步。' : '问题澄清后可以继续，压缩本身不触发换会话。'}<br /><span>结果反映本次评估，非实时质量保证。</span></p></div>}
        <div className="panel-actions">
          <button className="primary-button" onClick={health.level === 'unknown' ? () => setView('review') : prepareHandoff}>
            {health.level === 'unknown' ? '准备语义评估' : '整理干净交接'}<ArrowUpRight size={16} />
          </button>
          <button className="secondary-button" disabled={!snapshot} onClick={snooze}><BellOff size={14} />稍后提醒</button>
        </div>
        <footer className="panel-footer"><span><i />{health.reviewedAt ? `${isNative ? '评估于' : '演示评估'} ${new Date(health.reviewedAt).toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' })}` : '等待本地评估'}</span><button onClick={() => setView('settings')} aria-label="提醒设置"><Settings2 size={14} />设置</button></footer>
      </>}
      {view === 'sessions' && <div className="subview">
        <h2>只关注，你选中的会话</h2><p className="subview-intro">手动固定一个会话。后台任务的更新不会自动切换它。</p>
        <div className="session-list">
          {sessions.map(item => <button className={`session-option ${pinnedId === item.id ? 'selected' : ''}`} key={item.id} title={item.id} aria-label={`${item.title} · ${item.id}`} onClick={() => pinSession(item.id)}>
            <span className="session-avatar"><Layers size={17} /></span><span><strong>{item.title}</strong><small>{item.source === 'demo' ? '演示会话' : item.assessment ? '已有手动语义评估' : '已收到生命周期事件'}</small></span>{pinnedId === item.id ? <Check size={17} /> : <ChevronRight size={15} />}
          </button>)}
          {sessions.length === 0 && <div className="empty-state"><Layers size={28} /><strong>等待第一份评估</strong><p>启用仓库插件后，在目标 Codex 会话中请求 context-health 评估。</p><button className="text-button" onClick={() => setView('review')}>准备评估指令</button></div>}
        </div>
        {pinnedId && <button className="text-button" onClick={() => { setPinnedId(null); if (isNative) savePreference('orb:pinned', ''); setView('overview') }}>解除固定</button>}
        <p className="micro-note"><Info size={13} />自动跟随当前窗口仍在验证中。</p>
      </div>}
      {view === 'evidence' && <div className="subview evidence-view">
        <span className={`status-chip status-${health.level}`}><i />{health.label}</span>
        <h2>{health.level === 'unknown' ? '上次评估，仅供回顾' : '为什么这样建议'}</h2><p className="subview-intro">{health.level === 'unknown' ? '这份报告不能代表当前执行状态。核对新的输入与进展后，再作决定。' : '逐项核对实际影响。已解决的问题与正常需求调整，不应继续触发提醒。'}</p>
        {assessment && <p className="review-note">评估时间：{new Date(assessment.reviewed_at_ms).toLocaleString('zh-CN')}</p>}
        <div className="review-goal"><small>当前目标</small><p>{assessment?.current_goal ?? '待确认'}</p></div>
        <div className="evidence-list">{assessment?.signals.map(signal => <article className={`evidence-item ${signal.status === 'resolved' ? 'resolved' : ''}`} key={signal.id}>
          <header><strong>{SIGNAL_LABELS[signal.kind]}</strong><span>{signal.status === 'resolved' ? '已澄清' : '待处理'}</span></header><p>{signal.summary}</p>
          <div className="evidence-tags"><span>{signal.after_compaction ? '压缩后观察' : '较早记录'}</span><span>{signal.affects_next_step ? '影响下一步' : '暂不影响执行'}</span>{signal.recurrence === 'after_correction' && <span>纠正后复现</span>}</div>
          <ol>{signal.evidence.map((item, index) => <li key={`${item.ref}-${index}`}><small>{item.ref}</small><p>{item.note}</p></li>)}</ol>
        </article>)}</div>
        {!assessment?.signals.length && <div className="clear-result evidence-clear"><ShieldCheck size={22} /><span>本次评估未发现仍影响执行的冲突。</span></div>}
        <p className="review-note">{assessment?.review_note}</p>
        {!isNative && health.actionable.length > 0 && <button className="secondary-button full-width" onClick={simulateClarification}>模拟澄清并复查<Check size={15} /></button>}
        <button className="text-button review-again" onClick={() => setView('review')}>准备重新评估<ChevronRight size={14} /></button>
      </div>}
      {view === 'review' && <div className="subview handoff-view">
        <span className="status-chip status-unknown"><i />由你发起</span><h2>检查这段思路</h2>
        <p className="subview-intro">在目标 Codex 会话中发送这段指令。需要先启用仓库提供的 context-health 插件。</p>
        <label className="sr-only" htmlFor="review-text">语义评估指令</label><textarea id="review-text" readOnly value={reviewPrompt} />
        <button className="primary-button full-width" onClick={() => void copyText(reviewPrompt, '已复制。请在要检查的 Codex 会话中发送；评估尚未运行。')}><Copy size={15} />复制评估指令</button>
        <p className="micro-note">评估结果会保存最小化的本地摘要与证据位置。悬浮球会读取结果；当前尚无自动后台评估。</p>
      </div>}
      {view === 'handoff' && <div className="subview handoff-view">
        <span className="status-chip status-healthy"><i />只带走有效信息</span>
        <h2>把思路，重新理清</h2>
        <p className="subview-intro">保留当前目标、有效约束与已验证结论。先解决冲突，再把干净的下一步交给新会话。</p>
        <label className="sr-only" htmlFor="handoff-text">交接摘要模板</label>
        <textarea id="handoff-text" value={handoff} onChange={event => setHandoff(event.target.value)} spellCheck={false} />
        <button className="primary-button full-width" onClick={() => void copyText(handoff, '已复制。核实有效信息后，粘贴到新会话。')}><Copy size={15} />复制交接模板</button>
        <p className="micro-note">复制后由你开启新会话；当前任务继续保留。</p>
      </div>}
      {view === 'settings' && <div className="subview">
        <h2>恰好够用的提醒</h2><p className="subview-intro">让信息可见，让注意力留在工作上。</p>
        <div className="setting-row"><div><strong>外观</strong><small>与你的工作环境协调</small></div><button className="theme-toggle" onClick={() => setTheme(theme === 'light' ? 'dark' : 'light')} aria-label="切换面板主题">{theme === 'light' ? <Moon size={17} /> : <Sun size={17} />}</button></div>
        <div className="setting-row"><div><strong>稍后提醒</strong><small>{snoozed ? '此会话已暂停 10 分钟' : '需要安静时，暂停 10 分钟'}</small></div><button className="text-button" disabled={!snapshot} onClick={() => { if (snoozed && pinnedId) { setSnoozedSessions(current => ({ ...current, [pinnedId]: 0 })); setToast('此会话提醒已恢复。') } else snooze() }}>{snoozed ? '恢复' : '暂停'}</button></div>
        <div className="setting-row"><div><strong>提醒依据</strong><small>压缩后反复出现、仍影响执行的问题</small></div><button className="text-button" onClick={() => setView('review')}>复查</button></div>
        <div className="privacy-card"><LockKeyhole size={18} /><strong>保留依据，减少冗余</strong><p>悬浮球不上传对话。手动评估只保存必要的本地摘要与证据位置，未知信息继续标为未知。</p></div>
        <p className="micro-note">自动后台评估与系统通知待接入。当前展示有时效的手动评估结果，规则准确率尚未在真实任务中校准。</p>
      </div>}
    </section>}
    <div className="orb-bottom-row">
      {!expanded && !widgetSurface && <div className="resting-caption"><span>{snoozed ? '安静 10 分钟' : health.label}</span><ArrowDownLeft size={14} /></div>}
      <button
        ref={orbRef} className={`orb-button ${snoozed ? 'snoozed' : ''}`}
        aria-label={expanded ? '收起 Context Orb' : '展开 Context Orb'} aria-expanded={expanded}
        aria-controls="orb-panel" title="点击展开，拖动定位"
        onPointerDown={pointerDown} onPointerMove={pointerMove} onPointerUp={pointerUp} onPointerCancel={() => { pointer.current = null }}
        onClick={event => { if (event.detail === 0) { setExpanded(open => !open); setView('overview') } }}
      >
        <svg className="orb-ring" viewBox="0 0 80 80" aria-hidden="true">
          <circle className="orb-ring-base" cx="40" cy="40" r="36" />
          <circle className="orb-ring-fill" cx="40" cy="40" r="36" strokeDasharray={health.level === 'unknown' ? '4 14' : '62 13.4'} />
        </svg>
        <span className="orb-core">{snoozed ? <BellOff size={22} /> : <OrbMark />}</span>
        <span className="orb-status-dot" />
      </button>
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
        <div className="eyebrow"><span />WHEN CONTEXT LOSES THE THREAD</div>
        <h1>专注，<br />让思路<span className="serif-word">清楚。</span></h1>
        <p className="intro-copy">多次压缩之后，思路还清楚吗？<br />当旧信息开始干扰下一步，轻轻提醒你。</p>
        <div className="platform-row"><span><svg viewBox="0 0 20 20" aria-hidden="true"><path d="M13.4 3.7c.8-1 1-2 .9-2.7-1 .1-2.1.6-2.8 1.5-.7.8-1 1.9-.9 2.7 1 .1 2-.5 2.8-1.5ZM16.9 14.2c-.4.9-.6 1.3-1.1 2.1-.7 1-1.6 2.4-2.8 2.4-1.1 0-1.4-.7-2.9-.7s-1.8.7-2.9.7c-1.2 0-2-1.2-2.7-2.2C2.5 13.6 2 9.7 3.3 7.7c.9-1.4 2.3-2.1 3.6-2.1 1.2 0 2 .7 3 .7s1.6-.7 3-.7c1.1 0 2.4.6 3.3 1.7-2.9 1.6-2.4 5.6.7 6.9Z" fill="currentColor"/></svg>macOS</span><span><svg viewBox="0 0 20 20" aria-hidden="true"><path d="m2 4 7-1v6H2V4Zm8-1.2L18 2v7h-8V2.8ZM2 10h7v6l-7-1v-5Zm8 0h8v7l-8-1v-6Z" fill="currentColor"/></svg>Windows</span><span className="platform-stage">框架 v0.2</span></div>
        <div className="scenario-controls">
          <div className="section-label"><span>试试四种状态</span><span>01 — 04</span></div>
          <div className="scenario-grid" role="group" aria-label="演示状态">
            {(Object.keys(scenarioLabels) as HealthLevel[]).map((level, index) => <button key={level} className={`scenario-button level-${level} ${scenario === level ? 'active' : ''}`} aria-pressed={scenario === level} onClick={() => chooseScenario(level)}><span className="scenario-indicator" /><span>{scenarioLabels[level]}</span><small>0{index + 1}</small></button>)}
          </div>
        </div>
        <div className="design-principle"><span className="principle-line" /><p>留下有效信息，让下一步清楚。<br /><span>根据执行证据，决定交接时机。</span></p></div>
      </section>
      <section className="workspace-stage" ref={stageRef} aria-label="悬浮球交互预览">
        <div className="stage-caption"><span className="stage-live-dot" />你的工作空间<span>示意场景 · 非真实会话</span></div>
        <div className="mock-window" aria-hidden="true">
          <div className="mock-window-bar"><div className="traffic-lights"><i /><i /><i /></div><span>studio / atlas</span><MoreHorizontal size={16} /></div>
          <div className="mock-window-content"><aside className="mock-sidebar"><div className="mock-project"><Folder size={15} />atlas</div><div className="mock-new"><Plus size={14} />新会话</div><small>进行中</small><div className="mock-selected">设置页交互优化</div><div>API 重试边界检查</div><small>工作空间</small><div className="mock-file"><Folder size={13} />src</div><div className="mock-file nested">components</div><div className="mock-file nested">settings.tsx</div></aside><div className="mock-conversation"><div className="mock-thread-title">设置页交互优化<span>本地</span></div><div className="mock-user">把设置页的交互再收敛一下。</div><div className="mock-assistant"><span className="tiny-orb"><OrbMark /></span><div><strong>先让主操作更容易被找到。</strong><p>统一设置项的层级，保留清晰的反馈。<br />完成当前步骤后，再检查键盘导航。</p><div className="mock-code"><span><i>01</i><b>const</b> preferences = &#123;</span><span><i>02</i>&nbsp; theme: <em>'auto'</em>,</span><span><i>03</i>&nbsp; notifications: <em>'quiet'</em></span><span><i>04</i>&#125;</span></div><div className="mock-done"><Check size={13} />已整理当前交互</div></div></div><div className="mock-composer">继续这段工作…<span>↵</span></div></div></div>
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
    }}><button className="icon-button modal-close" onClick={() => setHelp(false)} aria-label="关闭设计说明"><X size={18} /></button><OrbMark /><h2 id="help-title">让下一步，重新清楚。</h2><p>这里演示多次压缩后的语义完整性检查。所有会话与证据均为虚构，可查看原因、模拟澄清并准备干净交接。</p><ul><li><strong>先看实际影响：</strong>旧指令、遗漏约束与冲突事实是否仍在干扰执行。</li><li><strong>再看能否恢复：</strong>单次偏差先澄清，纠正后仍反复出现才提高提醒。</li><li><strong>保留选择权：</strong>长会话可以继续，建议可以推迟，新会话由你开启。</li></ul><button className="primary-button full-width" onClick={() => setHelp(false)}>开始体验<ChevronRight size={16} /></button></section></div>}
  </div>
}
