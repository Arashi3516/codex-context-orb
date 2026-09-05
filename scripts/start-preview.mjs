#!/usr/bin/env node
import { spawn } from 'node:child_process'
import { existsSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('../', import.meta.url))
const [major, minor] = process.versions.node.split('.').map(Number)
if (major < 22 || (major === 22 && minor < 12)) {
  console.error('Context Orb preview requires Node.js 22.12 or later.')
  process.exit(1)
}
const npm = process.platform === 'win32' ? 'npm.cmd' : 'npm'
function run(args) {
  return new Promise((resolve, reject) => {
    const child = spawn(npm, args, { cwd: root, stdio: 'inherit', shell: process.platform === 'win32' })
    child.on('error', reject)
    child.on('exit', code => code === 0 ? resolve() : reject(new Error(`npm exited with ${code}`)))
  })
}
try {
  if (!existsSync(new URL('../node_modules/vite/package.json', import.meta.url))) await run(['ci'])
  console.log('Context Orb interactive preview: http://127.0.0.1:1427 (demo data)')
  await run(['run', 'dev'])
} catch (error) {
  console.error(error.message)
  process.exitCode = 1
}
