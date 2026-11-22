// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useCallback, useEffect, useState } from 'react'
import { CalendarDays, Clock3, RefreshCw, Save } from 'lucide-react'
import {
  clockInHostedAttendance,
  clockOutHostedAttendance,
  getMyHostedAttendance,
  listHostedAttendancePeriods,
  listHostedAttendanceSchedules,
  listHostedAttendanceStaff,
  saveHostedAttendanceSchedule,
} from './api'
import type { HostedAttendanceStaff, HostedMyAttendance, HostedWorkPeriod, HostedWorkSchedule } from './api'
import './HostedAttendancePanel.css'

const weekdays = [
  { value: 0, label: 'Mon' }, { value: 1, label: 'Tue' }, { value: 2, label: 'Wed' },
  { value: 3, label: 'Thu' }, { value: 4, label: 'Fri' }, { value: 5, label: 'Sat' }, { value: 6, label: 'Sun' },
]

type ScheduleDraft = { weekdays: number[]; startTime: string; endTime: string; enabled: boolean; expectedRevision: number }

function scheduleDraft(schedule?: HostedWorkSchedule): ScheduleDraft {
  return schedule
    ? { weekdays: schedule.weekdays, startTime: schedule.startTime, endTime: schedule.endTime, enabled: schedule.enabled, expectedRevision: schedule.revision }
    : { weekdays: [0, 1, 2, 3, 4], startTime: '08:00', endTime: '17:00', enabled: true, expectedRevision: 0 }
}

function displayTime(value: string, timezone: string): string {
  const date = new Date(value)
  if (Number.isNaN(date.valueOf())) return value
  return new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short', timeZone: timezone }).format(date)
}

function elapsedLabel(startedAt: string, now: number): string {
  const seconds = Math.max(0, Math.floor((now - new Date(startedAt).valueOf()) / 1000))
  const hours = Math.floor(seconds / 3600).toString().padStart(2, '0')
  const minutes = Math.floor((seconds % 3600) / 60).toString().padStart(2, '0')
  const remainder = (seconds % 60).toString().padStart(2, '0')
  return `${hours}:${minutes}:${remainder}`
}

export function HostedAttendancePanel({
  apiBaseUrl, accessToken, tenantId, branchId, branchName, canManage, canRead,
}: {
  apiBaseUrl: string
  accessToken: string
  tenantId: string
  branchId: string
  branchName: string
  canManage: boolean
  canRead: boolean
}) {
  const [mine, setMine] = useState<HostedMyAttendance | null>(null)
  const [staff, setStaff] = useState<HostedAttendanceStaff[]>([])
  const [periods, setPeriods] = useState<HostedWorkPeriod[]>([])
  const [drafts, setDrafts] = useState<Record<string, ScheduleDraft>>({})
  const [loading, setLoading] = useState(true)
  const [working, setWorking] = useState(false)
  const [savingMember, setSavingMember] = useState<string | null>(null)
  const [serverClockOffset, setServerClockOffset] = useState(0)
  const [monotonicNow, setMonotonicNow] = useState(() => performance.now())
  const [notice, setNotice] = useState<{ kind: 'error' | 'success'; text: string } | null>(null)

  const refresh = useCallback(async () => {
    setLoading(true)
    try {
      const attendance = await getMyHostedAttendance({ apiBaseUrl, accessToken, tenantId, branchId })
      setMine(attendance)
      setServerClockOffset(new Date(attendance.serverNow).valueOf() - performance.now())
      if (canManage || canRead) {
        const [branchStaff, schedules, branchPeriods] = await Promise.all([
          canManage ? listHostedAttendanceStaff({ apiBaseUrl, accessToken, tenantId, branchId }) : Promise.resolve([]),
          canManage ? listHostedAttendanceSchedules({ apiBaseUrl, accessToken, tenantId, branchId }) : Promise.resolve([]),
          canRead ? listHostedAttendancePeriods({ apiBaseUrl, accessToken, tenantId, branchId }) : Promise.resolve([]),
        ])
        setStaff(branchStaff)
        setPeriods(branchPeriods)
        const byMember = new Map(schedules.map((schedule) => [schedule.membershipId, schedule]))
        setDrafts(Object.fromEntries(branchStaff.map((person) => [person.membershipId, scheduleDraft(byMember.get(person.membershipId))])))
      } else {
        setStaff([])
        setPeriods([])
      }
      setNotice(null)
    } catch (reason) {
      setNotice({ kind: 'error', text: reason instanceof Error ? reason.message : 'Attendance could not be loaded.' })
    } finally {
      setLoading(false)
    }
  }, [accessToken, apiBaseUrl, branchId, canManage, canRead, tenantId])

  useEffect(() => { void refresh() }, [refresh])

  useEffect(() => {
    if (!mine?.currentPeriod) return
    const timer = window.setInterval(() => setMonotonicNow(performance.now()), 1000)
    return () => window.clearInterval(timer)
  }, [mine?.currentPeriod?.id, mine?.currentPeriod?.startedAt])

  async function clock() {
    setWorking(true)
    setNotice(null)
    try {
      if (mine?.currentPeriod) {
        await clockOutHostedAttendance({ apiBaseUrl, accessToken, tenantId, branchId })
        setNotice({ kind: 'success', text: 'You are clocked out. Your work period has been recorded.' })
      } else {
        await clockInHostedAttendance({ apiBaseUrl, accessToken, tenantId, branchId })
        setNotice({ kind: 'success', text: 'You are clocked in. Attendance is linked to your staff profile.' })
      }
      await refresh()
    } catch (reason) {
      setNotice({ kind: 'error', text: reason instanceof Error ? reason.message : 'Your attendance action could not be saved.' })
    } finally {
      setWorking(false)
    }
  }

  function updateDraft(memberId: string, change: (current: ScheduleDraft) => ScheduleDraft) {
    setDrafts((current) => ({ ...current, [memberId]: change(current[memberId] ?? scheduleDraft()) }))
  }

  async function saveSchedule(person: HostedAttendanceStaff) {
    const draft = drafts[person.membershipId]
    if (!draft) return
    setSavingMember(person.membershipId)
    setNotice(null)
    try {
      const saved = await saveHostedAttendanceSchedule({
        apiBaseUrl, accessToken, tenantId, branchId, membershipId: person.membershipId,
        ...draft,
      })
      setDrafts((current) => ({ ...current, [person.membershipId]: scheduleDraft(saved) }))
      setNotice({ kind: 'success', text: `Schedule saved for ${person.displayName?.trim() || 'staff member'}.` })
    } catch (reason) {
      setNotice({ kind: 'error', text: reason instanceof Error ? reason.message : 'The schedule could not be saved.' })
    } finally {
      setSavingMember(null)
    }
  }

  const ownPeriod = mine?.currentPeriod ?? null
  return <section className="hosted-attendance" aria-labelledby="hosted-attendance-title">
    <header className="hosted-attendance-heading">
      <div><p className="eyebrow">PEOPLE AND SHIFTS</p><h2 id="hosted-attendance-title">Attendance</h2><p>{branchName}</p></div>
      <button className="button button-quiet" type="button" onClick={() => void refresh()} disabled={loading}>
        <RefreshCw size={15}/>{loading ? 'Updating…' : 'Refresh'}
      </button>
    </header>

    {notice && <div className={`notice ${notice.kind === 'error' ? 'notice-danger' : 'notice-success'}`} role={notice.kind === 'error' ? 'alert' : 'status'}>{notice.text}</div>}

    <section className="attendance-self" aria-label="Your attendance">
      <div className="attendance-self-status"><span className="attendance-icon"><Clock3 size={18}/></span><div>
        <strong>{ownPeriod ? 'You are clocked in' : 'Your shift has not started'}</strong>
        {ownPeriod ? <small>Worked {elapsedLabel(ownPeriod.startedAt, serverClockOffset + monotonicNow)} · started {displayTime(ownPeriod.startedAt, ownPeriod.timezone)} · scheduled end {displayTime(ownPeriod.dueAt, ownPeriod.timezone)}</small>
          : <small>{mine?.schedule?.enabled ? `Your schedule starts at ${mine.schedule.startTime} (${mine.schedule.timezone}). Clocking in is manual.` : 'No shift will start automatically.'}</small>}
      </div></div>
      <button className={ownPeriod ? 'button button-quiet' : 'button button-primary'} type="button" onClick={() => void clock()} disabled={loading || working}>
        <Clock3 size={15}/>{working ? 'Saving…' : ownPeriod ? 'Clock out' : 'Clock in'}
      </button>
    </section>

    {mine?.schedule && <p className="attendance-policy-note"><CalendarDays size={14}/> Scheduled periods end automatically at their planned end time. Attendance follows your staff account if you temporarily switch to cashier mode.</p>}

    {canManage && <section className="attendance-management" aria-labelledby="attendance-schedules-title">
      <div className="attendance-subheading"><div><p className="eyebrow">BRANCH SETUP</p><h3 id="attendance-schedules-title">Staff schedules</h3></div><span>Set a schedule only for people who work shifts.</span></div>
      {staff.length === 0 && <p className="attendance-empty">No active branch staff profiles are available for scheduling.</p>}
      <div className="attendance-schedule-list">
        {staff.map((person) => {
          const draft = drafts[person.membershipId] ?? scheduleDraft()
          const name = person.displayName?.trim() || 'Staff member'
          return <article className="attendance-schedule-card" key={person.membershipId}>
            <div className="attendance-person"><strong>{name}</strong><small>{person.roles.join(' · ') || 'Staff profile'}</small></div>
            <label className="attendance-enabled"><input type="checkbox" checked={draft.enabled} onChange={(event) => {
              const enabled = event.currentTarget.checked
              updateDraft(person.membershipId, (current) => ({ ...current, enabled }))
            }}/> Schedule enabled</label>
            <fieldset className="attendance-weekdays"><legend>Working days</legend>{weekdays.map((day) => <label key={day.value}><input type="checkbox" checked={draft.weekdays.includes(day.value)} onChange={(event) => {
              const checked = event.currentTarget.checked
              updateDraft(person.membershipId, (current) => ({ ...current, weekdays: checked ? [...current.weekdays, day.value].sort() : current.weekdays.filter((value) => value !== day.value) }))
            }}/>{day.label}</label>)}</fieldset>
            <label className="attendance-time">Shift starts<input type="time" value={draft.startTime} onChange={(event) => updateDraft(person.membershipId, (current) => ({ ...current, startTime: event.currentTarget.value }))}/></label>
            <label className="attendance-time">Shift ends<input type="time" value={draft.endTime} onChange={(event) => updateDraft(person.membershipId, (current) => ({ ...current, endTime: event.currentTarget.value }))}/></label>
            <button className="button button-quiet attendance-save" type="button" onClick={() => void saveSchedule(person)} disabled={savingMember !== null || !draft.weekdays.length || draft.startTime === draft.endTime}>
              <Save size={14}/>{savingMember === person.membershipId ? 'Saving…' : draft.expectedRevision ? 'Save changes' : 'Set schedule'}
            </button>
          </article>
        })}
      </div>
    </section>}

    {canRead && <section className="attendance-history" aria-labelledby="attendance-history-title">
      <div className="attendance-subheading"><div><p className="eyebrow">RECENT ACTIVITY</p><h3 id="attendance-history-title">Branch work periods</h3></div><span>Most recent {periods.length} periods</span></div>
      {periods.length === 0 ? <p className="attendance-empty">No work periods have been recorded for this branch yet.</p> : <div className="attendance-table-wrap"><table>
        <thead><tr><th>Staff member</th><th>Started</th><th>Scheduled end</th><th>Ended</th><th>Status</th></tr></thead>
        <tbody>{periods.map((period) => {
          const person = staff.find((entry) => entry.membershipId === period.membershipId)
          return <tr key={period.id}><td>{person?.displayName?.trim() || 'Staff profile'}</td>
            <td>{displayTime(period.startedAt, period.timezone)}</td><td>{displayTime(period.dueAt, period.timezone)}</td>
            <td>{period.endedAt ? displayTime(period.endedAt, period.timezone) : '—'}</td>
            <td><span className={`attendance-status ${period.endedAt ? 'attendance-status-closed' : 'attendance-status-open'}`}>{period.endedAt ? period.endReason === 'schedule' ? 'Auto-ended' : 'Clocked out' : 'In progress'}</span></td></tr>
        })}</tbody>
      </table></div>}
    </section>}
  </section>
}
