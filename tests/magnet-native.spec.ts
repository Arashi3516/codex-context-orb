import { expect, test } from '@playwright/test'

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
  await expect(page.getByTestId('capacity-readout')).toContainText('未接入')
  await expect.poll(() => page.evaluate(() => (window as unknown as { magnetCalls: { command: string; args: { moved?: boolean } }[] }).magnetCalls.filter(call => call.command === 'end_magnetic_drag').at(-1)?.args.moved)).toBe(false)
  await expect(page.getByRole('progressbar')).toHaveCount(0)
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
  for (const height of [690, 360]) for (const corner of ['left-top', 'right-top', 'left-bottom', 'right-bottom']) {
    await page.setViewportSize({ width: 382, height })
    await page.goto(`/?surface=orb&corner=${corner}`)
    const orb = page.locator('.orb-button')
    await expect.poll(async () => (await orb.boundingBox())?.x).toBe(corner.includes('left') ? 6 : 296)
    await expect.poll(async () => (await orb.boundingBox())?.y).toBe(corner.includes('top') ? 6 : height - 86)
    const before = (await orb.boundingBox())!
    await orb.focus()
    await page.keyboard.press('Enter')
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
