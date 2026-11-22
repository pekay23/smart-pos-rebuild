// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import react from '@vitejs/plugin-react'
import tauriConfig from './src-tauri/tauri.conf.json' with { type: 'json' }
import { defineConfig } from 'vite'

// https://vite.dev/config/
export default defineConfig({
  plugins: [react()],
  define: { __APP_VERSION__: JSON.stringify(tauriConfig.version) },
  // Keep only the current bundle; old hashed assets must not accumulate.
  build: { emptyOutDir: true },
  clearScreen: false,
  server: {
    host: "127.0.0.1",
    port: 1420,
    strictPort: true,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
})
