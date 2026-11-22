// SPDX-License-Identifier: AGPL-3.0-or-later

import { Monitor, RefreshCw, ShieldCheck } from 'lucide-react'
import { useCallback, useEffect, useRef, useState } from 'react'
import { ApiError, getTenantStationCapacity } from './api'
import type { StationCapacitySnapshot } from './api'

export function TenantStationCapacityPanel({
  apiBaseUrl,
  accessToken,
  tenantId,
  tenantName,
}: {
  apiBaseUrl: string
  accessToken: string
  tenantId: string
  tenantName: string
}) {
  const [snapshot, setSnapshot] = useState<StationCapacitySnapshot | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const requestGeneration = useRef(0)

  const refresh = useCallback(async () => {
    const generation = requestGeneration.current + 1
    requestGeneration.current = generation
    setLoading(true)
    setError(null)
    try {
      const result = await getTenantStationCapacity({ apiBaseUrl, accessToken, tenantId })
      if (requestGeneration.current === generation) setSnapshot(result)
    } catch (reason) {
      const message = reason instanceof ApiError && reason.status === 403
        ? 'Only an owner can view this business license.'
        : reason instanceof Error ? reason.message : 'Station capacity could not be loaded.'
      if (requestGeneration.current === generation) {
        setError(message)
        setSnapshot(null)
      }
    } finally {
      if (requestGeneration.current === generation) setLoading(false)
    }
  }, [accessToken, apiBaseUrl, tenantId])

  useEffect(() => {
    void refresh()
    return () => { requestGeneration.current += 1 }
  }, [refresh])

  return <section className="tenant-station-capacity" aria-labelledby="station-capacity-title">
    <header>
      <span className="tenant-capacity-icon"><Monitor size={18}/></span>
      <div><p className="eyebrow">BUSINESS LICENSE</p><h2 id="station-capacity-title">Register stations</h2><p>{tenantName}</p></div>
      <button type="button" className="button button-quiet" onClick={() => void refresh()} disabled={loading} aria-label="Refresh station capacity">
        <RefreshCw size={15}/>{loading ? 'Updating…' : 'Refresh'}
      </button>
    </header>
    {error && <div className="notice notice-danger" role="alert">{error}</div>}
    {loading && !snapshot && !error && <p className="tenant-capacity-status" role="status">Loading licensed station capacity…</p>}
    {snapshot && <>
      {snapshot.entitlementActive && snapshot.stationLimit !== null
        ? <div className="tenant-capacity-metrics" aria-live="polite">
          <div><span>Device-linked stations</span><strong>{snapshot.activeStations} <small>of {snapshot.stationLimit}</small></strong></div>
          <div><span>Available station slots</span><strong>{snapshot.availableSlots}</strong></div>
        </div>
        : <div className="notice notice-warning" role="status">No active station capacity is configured for this business. Contact Advanced POS support to add a station license.</div>}
      <p className="tenant-capacity-note"><ShieldCheck size={14}/> Staff accounts are not billed as stations. Cashiers can take turns on a linked device with their own profiles; one person can transact on a station at a time.</p>
      <p className="tenant-capacity-disclosure">This is a capacity summary. Subscription changes and purchases are not available in the workspace yet.</p>
    </>}
  </section>
}
