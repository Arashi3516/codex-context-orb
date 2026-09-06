import { expect, test } from '@playwright/test'

test('procedural liquid keeps its canvas, blends interrupted states, and stops its clock', async ({ page }) => {
  const now = Date.now()
  await page.clock.install({ time: now })
  await page.clock.pauseAt(now + 1000)
  await page.goto('/')
  const visual = page.locator('.orb-visual')
  await expect(visual).toHaveAttribute('data-renderer', 'webgl')
  const canvas = page.locator('.orb-visual-canvas')
  const original = await canvas.elementHandle()
  const sample = () => canvas.evaluate(element => {
    const gl = (element as HTMLCanvasElement).getContext('webgl')!
    const program = gl.getParameter(gl.CURRENT_PROGRAM) as WebGLProgram
    return { tone: Array.from(gl.getUniform(program, gl.getUniformLocation(program, 'u_tone')) as Float32Array),
      time: (gl.getUniform(program, gl.getUniformLocation(program, 'u_phase_a')) as Float32Array)[0],
      error: gl.getError(), width: (element as HTMLCanvasElement).width }
  })
  const initial = await sample()
  expect(initial.error).toBe(0)
  expect(initial.width).toBeLessThanOrEqual(140)
  await page.getByRole('button', { name: '所列检查通过 01' }).click()
  await page.clock.runFor(400)
  const halfway = await sample()
  expect(halfway.tone[1]).toBeGreaterThan(0.35)
  expect(halfway.tone[1]).toBeLessThan(0.65)
  expect(halfway.tone[3]).toBeGreaterThan(0.35)
  expect(halfway.time).toBeGreaterThan(initial.time)
  await page.getByRole('button', { name: '疑点待核验 03' }).click()
  const interrupted = await sample()
  halfway.tone.forEach((value, index) => expect(Math.abs(value - interrupted.tone[index])).toBeLessThan(0.05))
  await page.clock.runFor(850)
  expect((await sample()).tone[2]).toBeCloseTo(1)
  await page.getByRole('button', { name: '关键证据缺失 04' }).click()
  await page.clock.runFor(850)
  expect((await sample()).tone[0]).toBeCloseTo(1)
  expect(await original!.evaluate(element => element.isConnected)).toBe(true)
  await page.getByRole('button', { name: '提醒设置' }).click()
  await page.getByRole('switch', { name: '球内流动效果' }).click()
  await expect(visual).toHaveAttribute('data-running', 'false')
  const stopped = await sample()
  await page.clock.runFor(1000)
  expect(await sample()).toEqual(stopped)
})
