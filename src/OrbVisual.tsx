import { memo, useEffect, useId, useRef, useState } from 'react'
import type { RiskTone } from './lib/presentation'
import { createLiquidRenderer, retargetLiquidTransition, sampleLiquidTransition,
  settledLiquidTransition, type LiquidRenderer } from './lib/orb-liquid'
import './orb-visual.css'

export interface OrbVisualProps {
  tone: RiskTone
  animated: boolean
  paused: boolean
  className?: string
}

/** One persistent canvas. React only handles props and lifecycle, never individual frames. */
export const OrbVisual = memo(function OrbVisual({ tone, animated, paused, className = '' }: OrbVisualProps) {
  const root = useRef<HTMLSpanElement>(null)
  const canvas = useRef<HTMLCanvasElement>(null)
  const options = useRef({ tone, animated, paused })
  options.current = { tone, animated, paused }
  const controller = useRef<{ update(): void } | null>(null)
  const [backend, setBackend] = useState<'fallback' | 'webgl'>('fallback')
  const [running, setRunning] = useState(false)

  useEffect(() => {
    const surface = canvas.current
    if (!surface) return
    const reducedMotion = matchMedia('(prefers-reduced-motion: reduce)')
    let renderer: LiquidRenderer | null = null
    let disposed = false
    let onScreen = true
    let frame: number | null = null
    let lastFrame: number | null = null
    let flowTime = 2.8
    let targetTone = options.current.tone
    let transition = settledLiquidTransition(targetTone, performance.now())
    let wasRunning = false

    const canRun = () => !!renderer && options.current.animated && !options.current.paused
      && !document.hidden && onScreen && !reducedMotion.matches
    const stop = () => {
      if (frame !== null) cancelAnimationFrame(frame)
      frame = null; lastFrame = null
      if (!disposed) setRunning(false)
    }
    const tick = (now: number) => {
      frame = null
      if (disposed || !canRun()) { stop(); return }
      if (lastFrame !== null) flowTime += Math.min(50, Math.max(0, now - lastFrame)) / 1000
      lastFrame = now
      renderer?.draw(flowTime, sampleLiquidTransition(transition, now))
      frame = requestAnimationFrame(tick)
    }
    const update = () => {
      if (disposed) return
      const now = performance.now()
      const shouldRun = canRun()
      if (targetTone !== options.current.tone) {
        targetTone = options.current.tone
        transition = shouldRun ? retargetLiquidTransition(transition, targetTone, now) : settledLiquidTransition(targetTone, now)
      } else if (wasRunning && !shouldRun) {
        const frozen = sampleLiquidTransition(transition, now)
        transition = { from: frozen, to: frozen, startedAt: now }
      } else if (!wasRunning && shouldRun) {
        transition = retargetLiquidTransition(transition, targetTone, now)
      }
      wasRunning = shouldRun
      renderer?.draw(flowTime, sampleLiquidTransition(transition, now))
      if (shouldRun) {
        if (frame === null) { lastFrame = null; frame = requestAnimationFrame(tick); setRunning(true) }
      } else stop()
    }
    const initialize = () => {
      if (disposed) return
      try {
        renderer?.dispose()
        renderer = createLiquidRenderer(surface)
        renderer.resize(devicePixelRatio)
        update()
        setBackend('webgl')
      } catch {
        renderer?.dispose(); renderer = null
        stop(); setBackend('fallback')
      }
    }
    const lost = (event: Event) => {
      event.preventDefault()
      stop()
      renderer?.dispose(); renderer = null
      wasRunning = false
      setBackend('fallback')
    }
    const resize = () => { renderer?.resize(devicePixelRatio); update() }
    const observer = typeof IntersectionObserver === 'undefined' ? null : new IntersectionObserver(entries => {
      onScreen = entries.some(entry => entry.isIntersecting)
      update()
    })
    surface.addEventListener('webglcontextlost', lost)
    surface.addEventListener('webglcontextrestored', initialize)
    document.addEventListener('visibilitychange', update)
    reducedMotion.addEventListener('change', update)
    window.addEventListener('resize', resize)
    if (root.current) observer?.observe(root.current)
    controller.current = { update }
    initialize()
    return () => {
      disposed = true
      stop()
      controller.current = null
      observer?.disconnect()
      surface.removeEventListener('webglcontextlost', lost)
      surface.removeEventListener('webglcontextrestored', initialize)
      document.removeEventListener('visibilitychange', update)
      reducedMotion.removeEventListener('change', update)
      window.removeEventListener('resize', resize)
      renderer?.dispose()
    }
  }, [])
  useEffect(() => { controller.current?.update() }, [tone, animated, paused])

  return <span ref={root} className={`orb-visual ${className}`.trim()} data-tone={tone}
    data-renderer={backend} data-running={running} aria-hidden="true">
    <canvas ref={canvas} className="orb-visual-canvas" data-testid="orb-liquid-canvas" hidden={backend !== 'webgl'} />
    {backend === 'fallback' && <OrbFallback tone={tone} />}
  </span>
})

const BACK_RIBBON = 'M28 7C60-2 90 17 75 39C63 56 24 62 30 83C13 62 25 46 52 34C78 23 54 10 28 7Z'
const FRONT_RIBBON = 'M72 7C93 29 60 40 36 54C17 65 25 83 64 94C28 96 8 77 22 56C34 39 79 25 72 7Z'
const RIBBON_EDGE = 'M72 7C93 29 60 40 36 54C17 65 25 83 64 94'

/** Static, code-drawn compatibility fallback for an unavailable or lost WebGL context. */
function OrbFallback({ tone }: { tone: RiskTone }) {
  const id = `orb-${useId().replace(/:/g, '')}`
  const fill = (name: string) => `url(#${id}-${name})`
  return <svg className="orb-visual-svg" viewBox="0 0 100 100" focusable="false">
      <defs>
        <clipPath id={`${id}-clip`}><circle cx="50" cy="50" r="48.4" /></clipPath>
        <radialGradient id={`${id}-shell`} cx="36%" cy="23%" r="82%">
          <stop offset="0" stopColor="var(--orb-shell-light)" />
          <stop offset=".38" stopColor="var(--orb-shell-mid)" />
          <stop offset=".75" stopColor="var(--orb-shell-dark)" />
          <stop offset="1" stopColor="#010617" />
        </radialGradient>
        <linearGradient id={`${id}-rim`} x1=".16" y1="0" x2=".8" y2="1">
          <stop stopColor="#dadfff" /><stop offset=".2" stopColor="#6873b3" />
          <stop offset=".48" stopColor="#0c183e" /><stop offset=".75" stopColor="#949dda" />
          <stop offset="1" stopColor="#283966" />
        </linearGradient>
        <linearGradient id={`${id}-ribbon-back`} x1=".2" y1="0" x2=".75" y2="1">
          <stop stopColor="var(--orb-band-pale)" stopOpacity=".88" />
          <stop offset=".25" stopColor="var(--orb-band-mid)" stopOpacity=".65" />
          <stop offset=".51" stopColor="var(--orb-band-deep)" stopOpacity=".17" />
          <stop offset=".72" stopColor="var(--orb-band-light)" stopOpacity=".82" />
          <stop offset="1" stopColor="var(--orb-band-deep)" stopOpacity=".2" />
        </linearGradient>
        <linearGradient id={`${id}-ribbon-front`} x1=".85" y1=".05" x2=".12" y2=".95">
          <stop stopColor="var(--orb-band-deep)" stopOpacity=".12" />
          <stop offset=".28" stopColor="var(--orb-band-light)" stopOpacity=".95" />
          <stop offset=".38" stopColor="var(--orb-band-pale)" />
          <stop offset=".48" stopColor="var(--orb-band-mid)" stopOpacity=".65" />
          <stop offset=".68" stopColor="var(--orb-band-deep)" stopOpacity=".16" />
          <stop offset=".86" stopColor="var(--orb-band-light)" stopOpacity=".9" />
          <stop offset="1" stopColor="var(--orb-band-pale)" />
        </linearGradient>
        <linearGradient id={`${id}-edge`} x1=".9" y1="0" x2=".1" y2="1">
          <stop stopColor="var(--orb-band-pale)" stopOpacity="0" />
          <stop offset=".3" stopColor="var(--orb-band-pale)" stopOpacity=".95" />
          <stop offset=".55" stopColor="var(--orb-band-light)" stopOpacity=".2" />
          <stop offset=".85" stopColor="var(--orb-band-light)" stopOpacity=".9" />
          <stop offset="1" stopColor="var(--orb-band-pale)" stopOpacity=".15" />
        </linearGradient>
        <radialGradient id={`${id}-halo`} cx="50%" cy="50%" r="50%">
          <stop offset=".58" stopColor="#7288ff" stopOpacity="0" />
          <stop offset=".89" stopColor="#576df8" stopOpacity=".16" />
          <stop offset=".98" stopColor="#b5baff" stopOpacity=".5" />
          <stop offset="1" stopColor="#fff" stopOpacity="0" />
        </radialGradient>
        <radialGradient id={`${id}-shine`} cx="34%" cy="8%" r="77%" gradientTransform="translate(0 .01) scale(1 .54)">
          <stop stopColor="#fff" stopOpacity=".93" /><stop offset=".17" stopColor="#d9e5ff" stopOpacity=".56" />
          <stop offset=".52" stopColor="#9ebaff" stopOpacity=".11" /><stop offset="1" stopColor="#9ebaff" stopOpacity="0" />
        </radialGradient>
        <radialGradient id={`${id}-fog`} cx="36%" cy="24%" r="78%">
          <stop stopColor="#e8ecf4" stopOpacity=".61" /><stop offset=".36" stopColor="#a4b0c6" stopOpacity=".45" />
          <stop offset=".77" stopColor="#728099" stopOpacity=".16" /><stop offset="1" stopColor="#43516c" stopOpacity="0" />
        </radialGradient>
        <linearGradient id={`${id}-amber`} x1="0" y1="1" x2="1" y2="0">
          <stop stopColor="#ad5d1a" stopOpacity="0" /><stop offset=".47" stopColor="#efad47" stopOpacity=".65" />
          <stop offset=".68" stopColor="#ffe5a0" /><stop offset="1" stopColor="#ef9d34" stopOpacity=".1" />
        </linearGradient>
      </defs>

      <circle cx="50" cy="50" r="49.2" fill="#101831" stroke={fill('rim')} strokeWidth="1" />
      <g clipPath={fill('clip')}>
        <circle cx="50" cy="50" r="48.4" fill={fill('shell')} />
        {tone === 'unknown' ? <g className="orb-visual-fog">
          <path d="M6 59C3 42 13 21 31 24C33 6 56 3 64 20C84 11 100 30 89 48C105 60 84 83 68 76C56 97 28 90 28 73C11 82 4 72 6 59Z" fill={fill('fog')} />
          <path d="M13 45C28 27 40 48 34 60C28 71 53 80 59 61C65 42 43 34 53 23C68 6 87 36 74 50C62 67 85 77 74 88C60 102 25 85 21 66C18 53 29 39 13 45Z" fill={fill('fog')} />
          <path d="M18 58C8 38 30 13 47 22C63 30 42 36 44 48C45 63 73 51 78 66C83 79 63 94 49 84C35 75 50 64 40 58C30 51 23 72 18 58Z" fill={fill('fog')} opacity=".72" />
          <g fill="none" stroke="#d0d8e6" strokeWidth=".65" opacity=".2">
            <path d="M16 48C18 29 38 23 44 34S33 58 48 68S69 63 73 52" />
            <path d="M29 19C51 7 66 25 60 38S68 48 79 41M21 63C16 76 34 85 42 78" />
          </g>
        </g> : <>
          <g className="orb-visual-flow orb-visual-flow-back">
            <path d={BACK_RIBBON} fill={fill('ribbon-back')} />
            <path d="M28 7C60-2 90 17 75 39C63 56 24 62 30 83" fill="none" stroke={fill('edge')} strokeWidth=".65" />
            <path d="M40 12C72 15 81 28 59 41C42 50 24 54 23 66" fill="none" stroke={fill('edge')} strokeWidth=".3" opacity=".6" />
          </g>
          <g className="orb-visual-flow orb-visual-flow-front">
            <path d={FRONT_RIBBON} fill={fill('ribbon-front')} />
            <path d={RIBBON_EDGE} fill="none" stroke={fill('edge')} strokeWidth="1.1" />
            <path d="M77 21C75 39 27 44 25 64C23 79 46 90 62 93" fill="none" stroke={fill('edge')} strokeWidth=".4" />
            <path d="M72 30C56 43 28 48 24 61M28 72C35 84 48 88 60 90" fill="none" stroke="#e7f7ff" strokeWidth=".25" opacity=".6" />
          </g>
          {tone === 'review' && <g className="orb-visual-interference">
            <path d="M77 17C94 39 60 45 45 55C61 37 82 35 77 17Z" fill={fill('amber')} />
            <path d="M80 26C84 39 62 43 52 49" fill="none" stroke="#ffe5a0" strokeWidth=".75" />
            <path d="M66 39L73 34M62 44L68 43M68 48L75 45" fill="none" stroke="#fbb64d" strokeWidth="1.1" strokeLinecap="round" />
            <circle cx="81" cy="47" r="1.2" fill="#ffd78a" /><circle cx="75" cy="55" r=".65" fill="#f5b95d" />
          </g>}
          {tone === 'deviation' && <g className="orb-visual-interference" fill="none" strokeLinecap="round" strokeLinejoin="round">
            <path d="M16 61C27 40 54 60 78 27M30 85C46 74 42 64 62 61C80 57 81 46 88 43" stroke="#ee716a" strokeWidth="1.8" opacity=".56" />
            <path d="M17 25L27 35L28 47L39 54M27 35L23 23L27 17M28 44L17 41L12 33M54 48L62 35L59 24L63 16M62 35L74 32L82 20M60 64L69 74L70 87M69 74L81 73L88 66" stroke="#ff9386" strokeWidth=".7" opacity=".9" />
            <path d="M32 48L39 47M50 59L57 58M65 46L71 39" stroke="#ffdbcc" strokeWidth="1.4" />
          </g>}
          <g fill="var(--orb-band-pale)" opacity=".48">
            <circle cx="24" cy="30" r=".45" /><circle cx="40" cy="23" r=".3" /><circle cx="68" cy="71" r=".5" />
            <circle cx="77" cy="63" r=".3" /><circle cx="46" cy="81" r=".4" /><circle cx="32" cy="72" r=".25" />
            <circle cx="81" cy="35" r=".4" /><circle cx="18" cy="55" r=".25" /><circle cx="56" cy="18" r=".3" />
          </g>
        </>}
        <circle cx="50" cy="50" r="47.6" fill={fill('halo')} />
        <circle cx="50" cy="50" r="47.5" fill={fill('shine')} />
        <path d="M14 24C28 4 61-2 81 16" fill="none" stroke="#e3e7ff" strokeWidth=".9" opacity=".76" />
        <path d="M25 88C46 100 73 93 85 78" fill="none" stroke="#859eff" strokeWidth=".8" opacity=".7" />
      </g>
  </svg>
}
