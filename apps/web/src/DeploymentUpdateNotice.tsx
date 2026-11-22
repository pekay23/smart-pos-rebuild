// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useCallback, useEffect, useState } from 'react'
import { RefreshCw } from 'lucide-react'

type BuildInfo = { buildId: string; version: string }
declare const __BUILD_ID__: string

export function DeploymentUpdateNotice() {
  const [available, setAvailable] = useState(false)
  const [dismissedUntil, setDismissedUntil] = useState(0)

  const check = useCallback(async () => {
    if (document.visibilityState !== 'visible') return
    try {
      const response = await fetch(`/build-info.json?check=${Date.now()}`, { cache: 'no-store', headers: { accept: 'application/json' } })
      if (!response.ok) return
      const latest = await response.json() as BuildInfo
      if (typeof latest.buildId === 'string' && latest.buildId !== __BUILD_ID__) setAvailable(true)
    } catch {
      // A temporary network failure should not interrupt the workspace.
    }
  }, [])

  useEffect(() => {
    const later = Number(sessionStorage.getItem('advanced-pos:update-remind-after') ?? 0)
    setDismissedUntil(Number.isFinite(later) ? later : 0)
    void check()
    const timer = window.setInterval(() => void check(), 5 * 60_000)
    const onVisible = () => { if (document.visibilityState === 'visible') void check() }
    document.addEventListener('visibilitychange', onVisible)
    return () => { window.clearInterval(timer); document.removeEventListener('visibilitychange', onVisible) }
  }, [check])

  useEffect(() => {
    if (dismissedUntil > 0 && Date.now() >= dismissedUntil) {
      setDismissedUntil(0)
      sessionStorage.removeItem('advanced-pos:update-remind-after')
    }
  }, [dismissedUntil])

  useEffect(() => {
    if (dismissedUntil <= Date.now()) return;
    const timer = window.setTimeout(() => {
      setDismissedUntil(0)
      sessionStorage.removeItem('advanced-pos:update-remind-after')
    }, dismissedUntil - Date.now())
    return () => window.clearTimeout(timer)
  }, [dismissedUntil])

  if (!available || Date.now() < dismissedUntil) return null
  return <aside className="deployment-update-notice" aria-label="App update available">
    <span className="deployment-update-icon"><RefreshCw size={17}/></span>
    <span className="deployment-update-copy"><strong role="status">A new version is ready</strong><small>Refresh to load the latest web app. Your open basket is saved on this device; re-enter checkout details that were not saved.</small></span>
    <button className="button button-primary button-small" type="button" onClick={() => window.location.reload()}><RefreshCw size={14}/>Refresh now</button>
    <button className="deployment-update-later" type="button" onClick={() => {
      const until = Date.now() + 15 * 60_000
      sessionStorage.setItem('advanced-pos:update-remind-after', String(until))
      setDismissedUntil(until)
    }}>Later</button>
  </aside>
}
