import { expect, test, type Page } from '@playwright/test'
import { demoReport, DEMO_PRIMARY_ID } from '../src/lib/demo'
import type { EvidenceReport } from '../src/lib/evidence'
import type { MagnetState } from '../src/lib/magnet'

type NativeCall = { command: string; args: { width?: number; height?: number; moved?: boolean }; completed: boolean }
type NativeWindow = Window & {
  nativeControl: {
    calls: NativeCall[]
    hold: string[]
    pending: { command: string; resolve: () => void; reject: () => void }[]
    release: (command: string, fail?: boolean) => void
    state: MagnetState
    reports: EvidenceReport[]
    present: boolean
  }
}

async function installControlledNative(page: Page, report?: EvidenceReport) {
  const now = Date.now()
  await page.clock.install({ time: now })
  await page.clock.pauseAt(now + 1000)
  await page.addInitScript(({ report, sessionId }) => {
    const w = window as unknown as NativeWindow & { __TAURI_INTERNALS__: object }
    if (report) localStorage.setItem('orb:pinned', sessionId)
    const state: MagnetState = {
      revision: 0, preferences: { windowMode: 'codex' },
      capabilities: { drag: 'available', screenEdges: 'available', windowEdges: 'available', coordinateSpace: 'logical_points', codexGui: 'bundle_id', reason: null },
      dragging: false, lastDragMoved: false, snappedX: null, snappedY: null, snapSideX: null, snapSideY: null,
      layoutAnchorLeft: false, layoutAnchorTop: false, windowTargets: 1,
      geometry: { x: -450, y: -120, width: 80, height: 80 },
      monitor: { workArea: { x: -1440, y: -900, width: 1440, height: 900 }, unitsPerLogicalPixel: 2 }, lastError: null,
    }
    const control = w.nativeControl = {
      calls: [] as NativeCall[], hold: [] as string[], pending: [] as NativeWindow['nativeControl']['pending'],
      state, reports: report ? [report] : [], present: true,
      release(command: string, fail = false) {
        this.hold = this.hold.filter(item => item !== command)
        const ready = this.pending.filter(item => item.command === command)
        this.pending = this.pending.filter(item => item.command !== command)
        for (const item of ready) if (fail) item.reject(); else item.resolve()
      },
    }
    w.__TAURI_INTERNALS__ = {
      invoke: async (command: string, args: NativeCall['args'] = {}) => {
        if (command.startsWith('read_')) {
          if (!control.present) return []
          if (command === 'read_evidence_reports') return structuredClone(control.reports)
          if (command === 'read_hook_events' && control.reports.length) return [{
            schema_version: 1, source: 'codex-hook', session_id: sessionId, turn_id: 'demo-turn-30',
            observed_at_ms: Date.now(), last_event_name: 'Stop', model: null,
            context_used_tokens: null, context_window_tokens: null, binding: 'unbound',
          }]
          return []
        }
        const call = { command, args, completed: false }
        control.calls.push(call)
        if (command !== 'get_magnet_state') state.revision++
        if (command === 'begin_magnetic_drag') state.dragging = true
        if (command === 'end_magnetic_drag') { state.dragging = false; state.lastDragMoved = !!args.moved }
        // Snapshot before delaying: the response can arrive after a newer poll.
        const reply = structuredClone(state)
        try {
          if (control.hold.includes(command)) await new Promise<void>((resolve, reject) => control.pending.push({
            command, resolve, reject: () => reject(new Error(`Synthetic ${command} failure`)),
          }))
          return reply
        } finally { call.completed = true }
      },
    }
  }, { report, sessionId: DEMO_PRIMARY_ID })
  await page.setViewportSize({ width: 382, height: 690 })
  await page.goto('/?surface=orb')
  await expect.poll(() => page.evaluate(() => (window as unknown as NativeWindow).nativeControl.calls.some(call => call.command === 'resize_orb_window' && call.completed))).toBe(true)
}

async function sizes(page: Page) {
  return page.evaluate(() => (window as unknown as NativeWindow).nativeControl.calls.filter(call => call.command === 'resize_orb_window').map(call => [call.args.width, call.args.height]))
}

test('delayed native replies cannot turn short drags into clicks or overwrite another setting', async ({ page }) => {
  await page.setViewportSize({ width: 382, height: 690 })
  await page.addInitScript(() => {
    const w = window as unknown as { __TAURI_INTERNALS__: object; magnetCalls: { command: string; args: unknown }[] }
    w.magnetCalls = []
    const state = {
      preferences: { windowMode: 'codex' },
      capabilities: { drag: 'available', screenEdges: 'available', windowEdges: 'available', coordinateSpace: 'logical_points', codexGui: 'bundle_id', reason: null },
      dragging: false, lastDragMoved: false, snappedX: null, snappedY: null, snapSideX: null, snapSideY: null,
      layoutAnchorLeft: false, layoutAnchorTop: false, windowTargets: 1, geometry: null, monitor: null, lastError: null,
    }
    w.__TAURI_INTERNALS__ = {
      invoke: async (command: string, args: { preferences?: typeof state.preferences }) => {
        if (command.startsWith('read_')) return []
        if (command !== 'get_magnet_state') w.magnetCalls.push({ command, args })
        if (command === 'begin_magnetic_drag') await new Promise(resolve => setTimeout(resolve, 250))
        if (command === 'set_magnet_preferences') {
          await new Promise(resolve => setTimeout(resolve, 150))
          state.preferences = args.preferences!
        }
        return structuredClone(state)
      },
    }
  })
  await page.goto('/?surface=orb')
  const orb = page.locator('.orb-button')
  const box = (await orb.boundingBox())!
  await page.mouse.move(box.x + 40, box.y + 40)
  await page.mouse.down()
  await page.mouse.move(box.x + 18, box.y + 40)
  await expect(page.locator('.orb-dock')).toHaveClass(/is-dragging/)
  await page.mouse.up()
  // A second gesture arrives before the first native begin response.
  await page.mouse.down()
  await page.mouse.move(box.x + 10, box.y + 40)
  await page.mouse.up()
  await expect.poll(() => page.evaluate(() => (window as unknown as { magnetCalls: { command: string }[] }).magnetCalls.filter(call => /magnetic_drag/.test(call.command)).map(call => call.command))).toEqual(['begin_magnetic_drag', 'end_magnetic_drag'])
  await expect(page.getByTestId('orb-panel')).toHaveCount(0)
  await expect.poll(() => page.evaluate(() => (window as unknown as { magnetCalls: { command: string; args: { moved?: boolean } }[] }).magnetCalls.find(call => call.command === 'end_magnetic_drag')?.args.moved)).toBe(true)
  // Motion after mouseup must not be added to the released gesture by a late IPC.
  await orb.click()
  await page.mouse.move(120, 180)
  await expect(page.getByRole('heading', { name: '只关注，你选中的会话' })).toBeVisible()
  await expect.poll(() => page.evaluate(() => (window as unknown as { magnetCalls: { command: string; args: { moved?: boolean } }[] }).magnetCalls.filter(call => call.command === 'end_magnetic_drag').at(-1)?.args.moved)).toBe(false)
  await orb.click({ button: 'right' })
  await expect(page.getByTestId('capacity-readout')).toContainText('未接入')
  await expect(page.getByRole('progressbar')).toHaveCount(0)
  await page.emulateMedia({ reducedMotion: 'reduce' })
  await orb.click()
  await expect.poll(() => page.evaluate(() => (window as unknown as { magnetCalls: { command: string; args: { reducedMotion?: boolean } }[] }).magnetCalls.filter(call => call.command === 'end_magnetic_drag').at(-1)?.args.reducedMotion)).toBe(true)
  await expect(page.getByTestId('orb-panel')).toHaveCount(0)
  await orb.click({ button: 'right' })
  await page.getByRole('button', { name: '提醒设置' }).click()
  const all = page.getByRole('radio', { name: '所有窗口' })
  const off = page.getByRole('radio', { name: '关闭', exact: true })
  await all.click()
  await expect(off).toBeDisabled()
  await expect(all).toBeChecked()
  await off.click()
  await expect(off).toBeChecked()
  await expect.poll(() => page.evaluate(() => (window as unknown as { magnetCalls: { command: string; args: { preferences: { windowMode: string } } }[] }).magnetCalls.filter(call => call.command === 'set_magnet_preferences').map(call => call.args.preferences.windowMode))).toEqual(['codex', 'all', 'off'])
})

test('each native layout corner keeps the orb fixed and the panel inside short viewports', async ({ page }) => {
  await page.addInitScript(() => {
    const corner = new URLSearchParams(location.search).get('corner') ?? ''
    const state = {
      preferences: { windowMode: 'codex' },
      capabilities: { drag: 'available', screenEdges: 'available', windowEdges: 'available', coordinateSpace: 'logical_points', codexGui: 'bundle_id', reason: null },
      dragging: false, lastDragMoved: false, snappedX: null, snappedY: null, snapSideX: null, snapSideY: null,
      layoutAnchorLeft: corner.includes('left'), layoutAnchorTop: corner.includes('top'),
      windowTargets: 0, geometry: null, monitor: null, lastError: null,
    }
    ;(window as unknown as { __TAURI_INTERNALS__: object }).__TAURI_INTERNALS__ = {
      invoke: async (command: string) => command.startsWith('read_') ? [] : structuredClone(state),
    }
  })
  for (const corner of ['left-top', 'right-top', 'left-bottom', 'right-bottom']) {
    await page.setViewportSize({ width: 92, height: 92 })
    await page.goto(`/?surface=orb&corner=${corner}`)
    const orb = page.locator('.orb-button')
    await orb.focus()
    const paint = await orb.evaluate(element => {
      const box = element.getBoundingClientRect()
      const style = getComputedStyle(element)
      const outside = Math.max(0, parseFloat(style.outlineOffset) + parseFloat(style.outlineWidth))
      return { focused: element.matches(':focus-visible'), width: parseFloat(style.outlineWidth),
        left: box.left - outside, top: box.top - outside,
        right: box.right + outside, bottom: box.bottom + outside }
    })
    expect(paint.focused).toBe(true)
    expect(paint.width).toBeGreaterThan(0)
    expect(paint.left).toBeGreaterThanOrEqual(0)
    expect(paint.top).toBeGreaterThanOrEqual(0)
    expect(paint.right).toBeLessThanOrEqual(92)
    expect(paint.bottom).toBeLessThanOrEqual(92)
  }
  for (const height of [690, 360]) for (const corner of ['left-top', 'right-top', 'left-bottom', 'right-bottom']) {
    await page.setViewportSize({ width: 382, height })
    await page.goto(`/?surface=orb&corner=${corner}`)
    const orb = page.locator('.orb-button')
    await expect.poll(async () => (await orb.boundingBox())?.x).toBe(corner.includes('left') ? 6 : 296)
    await expect.poll(async () => (await orb.boundingBox())?.y).toBe(corner.includes('top') ? 6 : height - 86)
    const before = (await orb.boundingBox())!
    await orb.focus()
    await expect(page.getByTestId('orb-peek')).toBeVisible()
    expect(await orb.boundingBox()).toEqual(before)
    const peek = (await page.getByTestId('orb-peek').boundingBox())!
    expect(peek.x).toBeGreaterThanOrEqual(0)
    expect(peek.y).toBeGreaterThanOrEqual(0)
    expect(peek.x + peek.width).toBeLessThanOrEqual(382)
    expect(peek.y + peek.height).toBeLessThanOrEqual(height)
    await page.keyboard.press('Shift+F10')
    const panel = page.getByTestId('orb-panel')
    await expect(panel).toBeVisible()
    expect(await orb.boundingBox()).toEqual(before)
    const box = (await panel.boundingBox())!
    expect(box.x).toBeGreaterThanOrEqual(0)
    expect(box.y).toBeGreaterThanOrEqual(0)
    expect(box.x + box.width).toBeLessThanOrEqual(382)
    expect(box.y + box.height).toBeLessThanOrEqual(height)
    await page.keyboard.press('Escape')
    await expect(panel).toHaveCount(0)
    expect(await orb.boundingBox()).toEqual(before)
  }
})

test('native peek, details and collapse request their logical sizes in order', async ({ page }) => {
  await installControlledNative(page)
  const orb = page.locator('.orb-button')
  await expect.poll(() => sizes(page)).toEqual([[92, 92]])
  await orb.hover()
  await page.clock.runFor(350)
  await expect(page.getByTestId('orb-peek')).toBeVisible()
  await expect.poll(() => sizes(page)).toEqual([[92, 92], [320, 240]])
  await orb.click({ button: 'right' })
  await expect(page.getByTestId('orb-panel')).toBeVisible()
  await expect.poll(() => sizes(page)).toEqual([[92, 92], [320, 240], [382, 690]])
  await page.keyboard.press('Escape')
  await expect.poll(() => sizes(page)).toEqual([[92, 92], [320, 240], [382, 690], [92, 92]])
  await expect(page.getByTestId('orb-peek')).toHaveCount(0)
  await expect(page.getByTestId('orb-panel')).toHaveCount(0)
})

test('a delayed drag keeps peek and resize paused through both begin and end replies', async ({ page }) => {
  await installControlledNative(page)
  await page.evaluate(() => { (window as unknown as NativeWindow).nativeControl.hold = ['begin_magnetic_drag', 'end_magnetic_drag'] })
  const orb = page.locator('.orb-button')
  const box = (await orb.boundingBox())!
  await page.mouse.move(box.x + 40, box.y + 40)
  await page.mouse.down()
  await page.mouse.move(box.x + 20, box.y + 40)
  await page.clock.runFor(500)
  await expect(page.locator('.orb-button .orb-visual')).toHaveAttribute('data-running', 'false')
  await expect(page.getByTestId('orb-peek')).toHaveCount(0)
  expect(await sizes(page)).toEqual([[92, 92]])
  await page.mouse.up()
  await page.evaluate(() => (window as unknown as NativeWindow).nativeControl.release('begin_magnetic_drag'))
  await expect.poll(() => page.evaluate(() => (window as unknown as NativeWindow).nativeControl.pending.map(item => item.command))).toContain('end_magnetic_drag')
  await page.clock.runFor(500)
  await page.mouse.down()
  await page.mouse.up()
  expect(await page.evaluate(() => (window as unknown as NativeWindow).nativeControl.calls.filter(call => call.command === 'begin_magnetic_drag').length)).toBe(1)
  expect(await sizes(page)).toEqual([[92, 92]])
  await page.evaluate(() => (window as unknown as NativeWindow).nativeControl.release('end_magnetic_drag'))
  await expect.poll(() => sizes(page)).toEqual([[92, 92], [92, 92]])
  await expect(page.getByTestId('orb-panel')).toHaveCount(0)
  await expect(page.getByTestId('orb-peek')).toHaveCount(0)
})

test('Escape during the keyboard state check still collapses after the reply', async ({ page }) => {
  await installControlledNative(page)
  const orb = page.locator('.orb-button')
  await orb.click({ button: 'right' })
  await expect.poll(() => sizes(page)).toEqual([[92, 92], [382, 690]])
  await page.evaluate(() => { (window as unknown as NativeWindow).nativeControl.hold = ['get_magnet_state'] })
  await orb.focus()
  await page.keyboard.press('Enter')
  await expect.poll(() => page.evaluate(() => (window as unknown as NativeWindow).nativeControl.pending.length)).toBeGreaterThan(0)
  await page.keyboard.press('Escape')
  await expect(page.getByTestId('orb-panel')).toHaveCount(0)
  expect(await sizes(page)).toEqual([[92, 92], [382, 690]])
  await page.evaluate(() => (window as unknown as NativeWindow).nativeControl.release('get_magnet_state'))
  await expect.poll(() => sizes(page)).toEqual([[92, 92], [382, 690], [92, 92]])
  await expect(page.getByTestId('orb-panel')).toHaveCount(0)
  await expect(page.getByRole('status')).toContainText('会话或检查依据已变化')
})

test('an old resize reply cannot overwrite a newer corner or discard a queued collapse', async ({ page }) => {
  await installControlledNative(page)
  await page.evaluate(() => { (window as unknown as NativeWindow).nativeControl.hold = ['resize_orb_window'] })
  const orb = page.locator('.orb-button')
  await orb.hover()
  await page.clock.runFor(350)
  await expect.poll(() => sizes(page)).toEqual([[92, 92], [320, 240]])
  // A host update wins over the earlier peek response, including on a negative-origin display.
  await page.evaluate(() => Object.assign((window as unknown as NativeWindow).nativeControl.state, { revision: 100, layoutAnchorLeft: true, layoutAnchorTop: true }))
  await page.clock.runFor(960)
  await expect(page.locator('.orb-dock')).toHaveClass(/dock-left/)
  await expect(page.locator('.orb-dock')).toHaveClass(/dock-top/)
  await orb.focus()
  await page.keyboard.press('Escape')
  expect(await sizes(page)).toEqual([[92, 92], [320, 240]])
  await page.evaluate(() => (window as unknown as NativeWindow).nativeControl.release('resize_orb_window'))
  await expect.poll(() => sizes(page)).toEqual([[92, 92], [320, 240], [92, 92]])
  await expect(page.locator('.orb-dock')).toHaveClass(/dock-left/)
  await expect(page.locator('.orb-dock')).toHaveClass(/dock-top/)
  await expect(page.getByTestId('orb-peek')).toHaveCount(0)
})

for (const command of ['begin_magnetic_drag', 'end_magnetic_drag', 'resize_orb_window']) {
  test(`a rejected ${command} releases the interaction guard without running a click`, async ({ page }) => {
    await installControlledNative(page)
    await page.evaluate(command => { (window as unknown as NativeWindow).nativeControl.hold = [command] }, command)
    const orb = page.locator('.orb-button')
    if (command === 'resize_orb_window') {
      await orb.hover()
      await page.clock.runFor(350)
      await expect.poll(() => page.evaluate(() => (window as unknown as NativeWindow).nativeControl.pending.length)).toBe(1)
      await orb.click()
      expect(await page.evaluate(() => (window as unknown as NativeWindow).nativeControl.calls.some(call => call.command === 'begin_magnetic_drag'))).toBe(false)
    } else {
      await orb.click()
      await expect.poll(() => page.evaluate(() => (window as unknown as NativeWindow).nativeControl.pending.map(item => item.command))).toContain(command)
    }
    await page.evaluate(command => (window as unknown as NativeWindow).nativeControl.release(command, true), command)
    await expect.poll(() => page.evaluate(command => (window as unknown as NativeWindow).nativeControl.calls.filter(call => call.command === command).every(call => call.completed), command)).toBe(true)
    await expect(page.getByTestId('orb-panel')).toHaveCount(0)
    await orb.click({ button: 'right' })
    await expect(page.getByTestId('orb-panel')).toBeVisible()
    await expect.poll(async () => (await sizes(page)).at(-1)).toEqual([382, 690])
  })
}

for (const change of ['basis', 'target']) {
  test(`a held orb click is cancelled when its ${change} changes during the native refresh`, async ({ page }) => {
    await installControlledNative(page, demoReport('failed'))
    const orb = page.locator('.orb-button')
    await expect(orb).toHaveAccessibleName('查看偏差依据 · Context Orb')
    const box = (await orb.boundingBox())!
    await page.mouse.move(box.x + 40, box.y + 40)
    await page.mouse.down()
    await page.evaluate(({ change, next }) => {
      const control = (window as unknown as NativeWindow).nativeControl
      if (change === 'target') control.present = false
      else control.reports = [next]
    }, { change, next: demoReport('passed') })
    await page.clock.runFor(3000)
    await expect(orb).toHaveAccessibleName(change === 'target' ? '选择会话 · Context Orb' : '查看会话状态 · Context Orb')
    await page.mouse.up()
    await expect(page.getByRole('status')).toContainText('会话或检查依据已变化')
    await expect(page.getByTestId('orb-panel')).toHaveCount(0)
    await expect(page.getByTestId('orb-peek')).toHaveCount(0)
  })
}

for (const input of ['pointer', 'keyboard']) {
  test(`the panel primary action preserves its pressed intent across refresh with ${input}`, async ({ page }) => {
    await installControlledNative(page, demoReport('failed'))
    await page.locator('.orb-button').click({ button: 'right' })
    const primary = page.getByTestId('primary-action')
    await expect(primary).toHaveText('查看偏差依据')
    if (input === 'pointer') {
      await primary.hover()
      await page.mouse.down()
    } else {
      await primary.focus()
      await page.keyboard.down('Space')
    }
    await page.evaluate(next => { (window as unknown as NativeWindow).nativeControl.reports = [next] }, demoReport('passed'))
    await page.clock.runFor(3000)
    await expect(primary).toHaveText('查看会话状态')
    if (input === 'pointer') {
      // Updated receipt text can move the button. Keep holding and follow that
      // same button so mouseup submits the captured intent instead of cancelling outside it.
      await primary.hover()
      await page.mouse.up()
    }
    else await page.keyboard.up('Space')
    await expect(page.getByRole('status')).toContainText('会话或检查依据已变化')
    await expect(page.getByTestId('orb-panel').locator('.risk-value h2')).toHaveText('所列依据一致')
    await expect(page.locator('.evidence-item')).toHaveCount(0)
  })
}
