// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import react from '@vitejs/plugin-react'
import appPackage from './package.json' with { type: 'json' }
import { defineConfig, type Plugin } from 'vite'

const appVersion = appPackage.version
const environment = (globalThis as typeof globalThis & { process?: { env?: Record<string, string | undefined> } }).process?.env ?? {}
const buildId = environment.VERCEL_GIT_COMMIT_SHA ?? environment.GITHUB_SHA ?? `${appVersion}-${Date.now()}`
const buildInfo = JSON.stringify({ version: appVersion, buildId })

const buildInfoPlugin: Plugin = {
  name: 'advanced-pos-build-info',
  configureServer(server) {
    server.middlewares.use('/build-info.json', (_request, response) => {
      response.setHeader('content-type', 'application/json; charset=utf-8')
      response.setHeader('cache-control', 'no-store, max-age=0')
      response.end(buildInfo)
    })
  },
  generateBundle() {
    this.emitFile({ type: 'asset', fileName: 'build-info.json', source: buildInfo })
  },
}

export default defineConfig({
  plugins: [react(), buildInfoPlugin],
  define: { __APP_VERSION__: JSON.stringify(appVersion), __BUILD_ID__: JSON.stringify(buildId) },
  // Keep only the current bundle; old hashed assets must not accumulate.
  build: { emptyOutDir: true },
  server: { strictPort: true, port: 5173 },
})
