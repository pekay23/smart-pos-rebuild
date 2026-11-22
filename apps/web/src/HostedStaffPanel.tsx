// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useCallback, useEffect, useRef, useState } from 'react'
import { RefreshCw, Users } from 'lucide-react'
import { ApiError, listHostedStaff } from './api'
import type { HostedStaffMember } from './api'
import { HostedEntityImage } from './HostedEntityImage'
import './HostedStaffPanel.css'

export function HostedStaffPanel({
  apiBaseUrl, accessToken, tenantId, branchId, tenantName,
}: {
  apiBaseUrl: string
  accessToken: string
  tenantId: string
  branchId: string
  tenantName: string
}) {
  const [staff, setStaff] = useState<HostedStaffMember[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const generation = useRef(0)

  const refresh = useCallback(async () => {
    const current = ++generation.current
    setLoading(true)
    setError(null)
    try {
      const next = await listHostedStaff({ apiBaseUrl, accessToken, tenantId })
      if (generation.current === current) setStaff(next)
    } catch (reason) {
      if (generation.current === current) {
        setStaff([])
        setError(reason instanceof ApiError && reason.status === 403
          ? 'Only an owner or administrator can view staff profiles.'
          : reason instanceof Error ? reason.message : 'Staff profiles could not be loaded.')
      }
    } finally {
      if (generation.current === current) setLoading(false)
    }
  }, [accessToken, apiBaseUrl, tenantId])

  useEffect(() => {
    void refresh()
    return () => { generation.current += 1 }
  }, [refresh])

  return <section className="tenant-staff-panel" aria-labelledby="hosted-staff-title">
    <header className="section-heading">
      <div><p className="eyebrow">PEOPLE AND ACCESS</p><h2 id="hosted-staff-title">Staff profiles</h2><p>{tenantName}</p></div>
      <button className="button button-quiet" type="button" onClick={() => void refresh()} disabled={loading}>
        <RefreshCw size={15}/>{loading ? 'Updating…' : 'Refresh'}
      </button>
    </header>
    {error && <div className="notice notice-danger" role="alert">{error}</div>}
    {loading && staff.length === 0 && !error && <p role="status">Loading staff profiles…</p>}
    {!loading && staff.length === 0 && !error && <div className="empty-inline"><Users size={17}/>No active staff profiles are assigned to this business.</div>}
    {staff.length > 0 && <ul className="hosted-staff-list">
      {staff.map((person) => {
        const name = person.displayName?.trim() || 'Staff member'
        return <li key={person.membershipId}>
          <HostedEntityImage
            apiBaseUrl={apiBaseUrl}
            accessToken={accessToken}
            tenantId={tenantId}
            branchId={branchId}
            entityType="staff"
            entityId={person.membershipId}
            label={name}
            canEdit
          />
          <span className="hosted-staff-copy"><strong>{name}</strong><small>{person.roles.length ? person.roles.join(' · ') : 'No role assigned'}</small></span>
        </li>
      })}
    </ul>}
    <p className="tenant-staff-note">Only owners and administrators can manage these profiles and images.</p>
  </section>
}
