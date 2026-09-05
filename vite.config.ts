import { defineConfig } from 'vitest/config'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  test: { include: ['src/**/*.test.ts'] },
  server: { strictPort: true, host: '127.0.0.1', port: 1427 },
  build: { target: ['es2022', 'chrome110', 'safari15'] },
})
