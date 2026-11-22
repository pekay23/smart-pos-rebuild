// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useCallback, useEffect, useState, type FormEvent } from 'react'
import { Building2, Check, Clock3, Plus, RefreshCw, Save, Trash2 } from 'lucide-react'
import { ApiError, getHostedBranchProfile, updateHostedBranchProfile } from './api'
import type { BranchOpeningHours, HostedBranchProfile } from './api'

const DAYS = [
  ['monday', 'Monday'], ['tuesday', 'Tuesday'], ['wednesday', 'Wednesday'],
  ['thursday', 'Thursday'], ['friday', 'Friday'], ['saturday', 'Saturday'], ['sunday', 'Sunday'],
] as const

type Notice = { kind: 'success' | 'error'; text: string }

export function BranchProfilePanel({ apiBaseUrl, accessToken, tenantId, branchId, branchName, canManage }: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; branchName: string; canManage: boolean
}) {
  const [profile, setProfile] = useState<HostedBranchProfile | null>(null)
  const [address, setAddress] = useState('')
  const [phone, setPhone] = useState('')
  const [email, setEmail] = useState('')
  const [openingHours, setOpeningHours] = useState<BranchOpeningHours | null>(null)
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [notice, setNotice] = useState<Notice | null>(null)

  const load = useCallback(async () => {
    setLoading(true)
    try {
      const result = await getHostedBranchProfile({ apiBaseUrl, accessToken, tenantId, branchId })
      setProfile(result)
      setAddress(result.address ?? '')
      setPhone(result.phone ?? '')
      setEmail(result.email ?? '')
      setOpeningHours(result.openingHours)
      setNotice(null)
    } catch (reason) {
      setNotice({ kind: 'error', text: reason instanceof Error ? reason.message : 'Branch details could not be loaded.' })
    } finally {
      setLoading(false)
    }
  }, [accessToken, apiBaseUrl, branchId, tenantId])

  useEffect(() => { void load() }, [load])

  function updateInterval(day: keyof BranchOpeningHours, index: number, field: 'open' | 'close', value: string) {
    setOpeningHours((current) => current ? ({
      ...current,
      [day]: current[day].map((interval, intervalIndex) => intervalIndex === index ? { ...interval, [field]: value } : interval),
    }) : current)
    setNotice(null)
  }

  function addInterval(day: keyof BranchOpeningHours) {
    setOpeningHours((current) => current ? ({
      ...current,
      [day]: [...current[day], { open: '09:00', close: '17:00' }],
    }) : current)
    setNotice(null)
  }

  function removeInterval(day: keyof BranchOpeningHours, index: number) {
    setOpeningHours((current) => current ? ({
      ...current,
      [day]: current[day].filter((_, intervalIndex) => intervalIndex !== index),
    }) : current)
    setNotice(null)
  }

  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!profile || !openingHours) return
    setSaving(true)
    setNotice(null)
    try {
      const updated = await updateHostedBranchProfile({
        apiBaseUrl, accessToken, tenantId, branchId,
        address: address.trim() || null,
        phone: phone.trim() || null,
        email: email.trim() || null,
        openingHours,
        expectedRevision: profile.settingsRevision,
      })
      setProfile(updated)
      setOpeningHours(updated.openingHours)
      setNotice({ kind: 'success', text: `${branchName} details were saved.` })
    } catch (reason) {
      const stale = reason instanceof ApiError && reason.status === 409
      setNotice({ kind: 'error', text: stale
        ? 'These branch details changed in another session. Reload them before saving again.'
        : reason instanceof Error ? reason.message : 'Branch details could not be saved.' })
    } finally {
      setSaving(false)
    }
  }

  return <section className="branch-profile-panel" aria-labelledby="branch-profile-title">
    <header className="branch-profile-header">
      <div className="branch-profile-title"><span className="branch-profile-icon"><Building2 size={18}/></span><div><p className="eyebrow">BRANCH DETAILS</p><h2 id="branch-profile-title">{branchName}</h2><p>Contact details and local opening hours for this location.</p></div></div>
      <button className="button button-quiet" type="button" onClick={() => void load()} disabled={loading || saving}><RefreshCw size={15}/> Refresh</button>
    </header>
    {notice && <div className={`branch-profile-notice ${notice.kind}`} role={notice.kind === 'error' ? 'alert' : 'status'}>{notice.kind === 'success' && <Check size={15}/>} {notice.text}</div>}
    {loading ? <p className="branch-profile-state">Loading branch details…</p> : openingHours && <form onSubmit={(event) => void save(event)}>
      <div className="branch-profile-contact">
        <label>Street address<textarea maxLength={500} rows={2} value={address} onChange={(event) => setAddress(event.target.value)} disabled={!canManage || saving} placeholder="Address customers can use to find this branch"/></label>
        <label>Phone<input type="tel" maxLength={40} value={phone} onChange={(event) => setPhone(event.target.value)} disabled={!canManage || saving} placeholder="Branch contact number"/></label>
        <label>Email<input type="email" maxLength={254} value={email} onChange={(event) => setEmail(event.target.value)} disabled={!canManage || saving} placeholder="Branch contact email"/></label>
      </div>
      <div className="branch-hours-heading"><div><h3><Clock3 size={17}/> Opening hours</h3><p>Times use this branch’s configured timezone. Leave a day empty when the branch is closed.</p></div></div>
      <div className="branch-hours-list">{DAYS.map(([day, label]) => <div className="branch-hours-day" key={day}>
        <strong>{label}</strong>
        <div className="branch-hours-intervals">{openingHours[day].length === 0 && <span className="branch-hours-closed">Closed</span>}
          {openingHours[day].map((interval, index) => <div className="branch-hours-interval" key={`${day}-${index}`}>
            <label><span className="sr-only">{label} opening time, interval {index + 1}</span><input type="time" value={interval.open} onChange={(event) => updateInterval(day, index, 'open', event.target.value)} disabled={!canManage || saving}/></label>
            <span aria-hidden="true">to</span>
            <label><span className="sr-only">{label} closing time, interval {index + 1}</span><input type="time" value={interval.close} onChange={(event) => updateInterval(day, index, 'close', event.target.value)} disabled={!canManage || saving}/></label>
            {canManage && <button className="branch-hours-remove" type="button" aria-label={`Remove ${label} interval ${index + 1}`} onClick={() => removeInterval(day, index)} disabled={saving}><Trash2 size={15}/></button>}
          </div>)}
        </div>
        {canManage && <button className="branch-hours-add" type="button" onClick={() => addInterval(day)} disabled={saving || openingHours[day].length >= 4}><Plus size={14}/><span>Add hours</span></button>}
      </div>)}</div>
      {canManage && <div className="branch-profile-actions"><button className="button button-primary" type="submit" disabled={saving || !profile}>{saving ? 'Saving…' : <><Save size={15}/> Save branch details</>}</button></div>}
    </form>}
  </section>
}
