// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useEffect, useState, type FormEvent } from "react";
import { Fingerprint, Plus, Trash2 } from "lucide-react";
import { accountApi, workDuration, type Attendance, type PersonalPasskey, type WorkSchedule } from "../lib/accountApi";
import { posApi } from "../lib/api";
import { getErrorMessage } from "../lib/errors";
import type { SessionUser, StaffProfile } from "../lib/models";
import { UpdateCenter } from "../components/UpdateCenter";

const days = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const displayTime = (value: string) => new Date(value).toLocaleString();

export function AccountPage({ user, onSignOut }: { user: SessionUser; onSignOut: () => void }) {
  const [keys, setKeys] = useState<PersonalPasskey[]>([]);
  const [supported, setSupported] = useState(false);
  const [attendance, setAttendance] = useState<Attendance | null>(null);
  const [staff, setStaff] = useState<StaffProfile[]>([]);
  const [label, setLabel] = useState("");
  const [password, setPassword] = useState("");
  const [personalDevice, setPersonalDevice] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");
  const [schedule, setSchedule] = useState<WorkSchedule>({ userId: user.userId, timezone: "Africa/Accra", startTime: "08:00", endTime: "17:00", weekdays: [0,1,2,3,4] });
  const canSchedule = user.permissions.includes("users:write");
  const canReport = canSchedule || user.permissions.includes("reports:read");
  useEffect(() => {
    let active = true;
    let initialAttendance = true;
    void Promise.all([accountApi.passkeys(), accountApi.passkeysAvailable(), canSchedule ? posApi.staffProfiles() : Promise.resolve([])]).then(([rows, available, profiles]) => {
      if (active) { setKeys(rows); setSupported(available); setStaff(profiles); }
    }).catch((reason: unknown) => { if (active) setError(getErrorMessage(reason, "Account settings could not be loaded.")); });
    const refresh = () => void accountApi.attendance().then((data) => {
      if (!active) return;
      setAttendance(data);
      if (initialAttendance) {
        const saved = data.schedules.find((item) => item.userId === user.userId);
        if (saved) setSchedule(saved);
        initialAttendance = false;
      }
    }).catch((reason: unknown) => { if (active) setError(getErrorMessage(reason, "Attendance could not be loaded.")); });
    refresh();
    const timer = window.setInterval(refresh, 15_000);
    return () => { active = false; window.clearInterval(timer); };
  }, [canSchedule, user.userId]);

  async function run(action: () => Promise<void>, success: string) {
    setBusy(true); setError(""); setMessage("");
    try { await action(); setMessage(success); }
    catch (reason: unknown) { setError(getErrorMessage(reason, "The change could not be saved.")); }
    finally { setBusy(false); }
  }
  async function enroll(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    await run(async () => {
      await accountApi.registerPasskey(label, password, personalDevice);
      setPassword(""); setLabel(""); setKeys(await accountApi.passkeys());
    }, "Passkey added. Next time, enter your username and choose Sign in with passkey.");
  }
  async function remove(id: string) {
    await run(async () => { await accountApi.removePasskey(id, password); setPassword(""); setKeys(await accountApi.passkeys()); }, "Passkey removed from this account.");
  }
  const own = attendance?.periods.find((period) => period.userId === user.userId);
  return <section className="settings-page account-page">
    <div className="page-heading"><div><p className="eyebrow">YOUR WORKSPACE</p><h1>My account & work</h1><p className="page-subtitle">{user.fullName} · {user.roleName}</p></div></div>
    <UpdateCenter />
    {error && <p className="form-error" role="alert">{error}</p>}
    {message && <p className="settings-success" role="status">{message}</p>}
    <article className="settings-panel">
      <h2>Your work period</h2>
      <p className="account-timer-value">{own ? workDuration(own.elapsedSeconds) : "Not started"}<small>{own?.endedAt ? "Work ended" : own ? "Work in progress" : user.workspaceMode === "cashier" ? "Sign-in starts work during your scheduled hours" : "Start work when you begin a shift"}</small></p>
      {own && <p className="settings-hint">Started {displayTime(own.startedAt)} · {own.endedAt ? `Ended ${displayTime(own.endedAt)}` : `Automatic end ${displayTime(own.dueAt)}`} · Late by {workDuration(own.lateSeconds)}</p>}
      <p>Signing out keeps an unfinished work period running. End work when you finish your day. Attendance closing does not reconcile the cash drawer.</p>
      {!attendance?.schedules.some((item) => item.userId === user.userId) && <p className="settings-hint">{user.workspaceMode === "cashier" ? "No work schedule is assigned. Sign-in starts an unscheduled period that stops after 16 hours as a safety limit. Ask an administrator to set your normal hours." : "No work schedule is assigned. Management sign-in does not start a work period; choose Start work when you begin a shift, or ask an administrator to set your normal hours."}</p>}
      <div className="settings-actions">
        <button className="button button-secondary" disabled={busy || Boolean(own && !own.endedAt)} onClick={() => void run(async () => { await accountApi.startWork(); setAttendance(await accountApi.attendance()); }, "Work started at the current time.")}>Start work</button>
        <button className="button button-primary" disabled={busy || !own || Boolean(own.endedAt)} onClick={() => void run(async () => { await accountApi.endWork(); setAttendance(await accountApi.attendance()); onSignOut(); }, "Work ended.")}>End work & sign out</button>
      </div>
    </article>
    <article className="settings-panel">
      <div className="settings-panel-heading"><Fingerprint size={22}/><div><h2>Personal passkeys</h2><p>Register your own authenticator for passwordless sign-in.</p></div></div>
      <p>On a shared till, use your personal security key or a personal authenticator offered by Windows. A shared Windows Hello PIN identifies the Windows account, so other staff who know it could use its passkeys.</p>
      {!supported && <p className="settings-hint">Native passkey setup is available in the Windows app. Use your password on other builds.</p>}
      <form onSubmit={(event) => void enroll(event)} className="account-form">
        <label className="field-label">Passkey name<input className="text-input" value={label} onChange={(event) => setLabel(event.target.value)} placeholder="My personal security key" maxLength={80} required/></label>
        <label className="field-label">Current account password<input className="text-input" type="password" autoComplete="current-password" value={password} onChange={(event) => setPassword(event.target.value)} required/><small>Required to add or remove a passkey.</small></label>
        <label className="account-checkbox"><input type="checkbox" checked={personalDevice} onChange={(event) => setPersonalDevice(event.target.checked)}/>This Windows account is mine alone. Allow Windows Hello on this device.</label>
        <button className="button button-primary" disabled={!supported || busy}><Plus size={16}/>{busy ? "Please wait…" : "Add passkey"}</button>
      </form>
      <div className="account-key-list">{keys.map((key) => <div className="setting-row" key={key.id}><span className="setting-copy"><strong>{key.label}</strong><small>Added {displayTime(key.createdAt)} · {key.lastUsedAt ? `Used ${displayTime(key.lastUsedAt)}` : "Not used yet"}</small></span><button className="button button-secondary button-small" disabled={busy || !password} onClick={() => void remove(key.id)} aria-label={`Remove ${key.label}`}><Trash2 size={15}/>Remove</button></div>)}</div>
      <p className="settings-hint">These passkeys belong to this local installation. Enroll separately on other installations until business account sync is available. Keep your password for recovery.</p>
    </article>
    {canSchedule && <article className="settings-panel">
      <h2>Staff work schedules</h2><p>Times use the selected business timezone. Overnight periods are supported. Changes apply to the next work period; an open period keeps its saved end time.</p>
      <form className="account-form" onSubmit={(event) => { event.preventDefault(); void run(async () => { await accountApi.saveSchedule(schedule); setAttendance(await accountApi.attendance()); }, "Work schedule saved for the next work period."); }}>
        <label className="field-label">Staff member<select className="text-input" value={schedule.userId} onChange={(event) => {
          const saved = attendance?.schedules.find((item) => item.userId === event.target.value);
          setSchedule(saved ?? { ...schedule, userId: event.target.value });
        }}>{staff.map((person) => <option key={person.userId} value={person.userId}>{person.fullName} · {person.roleName}</option>)}</select></label>
        <label className="field-label">Timezone<input className="text-input" value={schedule.timezone} onChange={(event) => setSchedule({ ...schedule, timezone: event.target.value })} required/></label>
        <div className="account-times"><label className="field-label">Starts<input className="text-input" type="time" value={schedule.startTime} onChange={(event) => setSchedule({ ...schedule, startTime: event.target.value })} required/></label><label className="field-label">Ends<input className="text-input" type="time" value={schedule.endTime} onChange={(event) => setSchedule({ ...schedule, endTime: event.target.value })} required/></label></div>
        <fieldset className="account-weekdays"><legend>Working days</legend>{days.map((day, index) => <label key={day}><input type="checkbox" checked={schedule.weekdays.includes(index)} onChange={(event) => setSchedule({ ...schedule, weekdays: event.target.checked ? [...schedule.weekdays, index] : schedule.weekdays.filter((value) => value !== index) })}/>{day}</label>)}</fieldset>
        <button className="button button-primary" disabled={busy || schedule.weekdays.length === 0}>Save schedule</button>
      </form>
      {attendance?.schedules.map((item) => <p className="settings-hint" key={item.userId}>{staff.find((person) => person.userId === item.userId)?.fullName ?? item.userId}: {item.startTime}–{item.endTime}, {item.weekdays.map((day) => days[day]).join(", ")} · {item.timezone}</p>)}
    </article>}
    <article className="settings-panel"><h2>{canReport ? "Store attendance" : "Your attendance"}</h2><p>Most recent 200 records. Scheduled starts are expectations; actual hours begin at sign-in.</p>
      <div className="table-scroll"><table><thead><tr><th>Staff</th><th>Signed in</th><th>End</th><th>Worked</th><th>Late</th><th>Closed by</th></tr></thead><tbody>{attendance?.periods.map((period) => <tr key={period.id}><td>{period.fullName}</td><td>{displayTime(period.startedAt)}</td><td>{period.endedAt ? displayTime(period.endedAt) : "Running"}</td><td>{workDuration(period.elapsedSeconds)}</td><td>{workDuration(period.lateSeconds)}</td><td>{period.endReason?.replace("_", " ") ?? "—"}</td></tr>)}</tbody></table></div>
    </article>
  </section>;
}
