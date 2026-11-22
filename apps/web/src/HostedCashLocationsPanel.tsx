// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useCallback, useEffect, useState } from 'react'
import type { FormEvent } from 'react'
import { ArrowLeftRight, Banknote, History, Plus, RefreshCw } from 'lucide-react'
import { createHostedCashLocation, getHostedCashLocationEvents, getHostedCashLocations, getHostedCashReconciliationStatus, openHostedCashLocation, postHostedCashReconciliation, transferHostedCash } from './api'
import type { HostedCashLocation, HostedCashLocationEvent, HostedCashLocationWorkspace, HostedCashReconciliationStatus, StationAccess } from './api'

function minorAmount(value: string, allowZero = false): number | null {
  if (!/^\d+(?:\.\d{1,2})?$/.test(value.trim())) return null
  const [whole = '', fraction = ''] = value.trim().split('.')
  const result = Number(whole) * 100 + Number(fraction.padEnd(2, '0'))
  return Number.isSafeInteger(result) && (allowZero ? result >= 0 : result > 0) ? result : null
}

function money(currency: string, amount: number): string {
  return new Intl.NumberFormat(undefined, { style: 'currency', currency }).format(amount / 100)
}

function eventLabel(event: HostedCashLocationEvent): string {
  if (event.eventType === 'opening') return 'Opening count'
  if (event.eventType === 'expense_outflow') return 'Branch expense'
  if (event.eventType === 'supplier_payment_outflow') return 'Supplier payment'
  if (event.eventType === 'expense_reimbursement_inflow') return 'Expense reimbursement received'
  if (event.eventType === 'sale_tender_inflow') return 'Cash sale received'
  if (event.eventType === 'sale_refund_outflow') return 'Customer cash refund'
  if (event.eventType === 'register_movement_inflow') return 'Cash paid into register'
  if (event.eventType === 'register_movement_outflow') return 'Cash removed from register'
  if (event.eventType === 'cash_drop_inflow') return 'Cash drop received from register'
  return event.eventType === 'transfer_in' ? 'Transfer received' : 'Transfer sent'
}

export function HostedCashLocationsPanel({ apiBaseUrl, accessToken, tenantId, branchId, branchName, stations, canManage, canReconcile }: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; branchName: string; stations: StationAccess[]; canManage: boolean; canReconcile: boolean
}) {
  const [workspace, setWorkspace] = useState<HostedCashLocationWorkspace | null>(null)
  const [reconciliation, setReconciliation] = useState<HostedCashReconciliationStatus | null>(null)
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState(false)
  const [notice, setNotice] = useState<{ kind: 'success' | 'error'; text: string } | null>(null)
  const [name, setName] = useState('')
  const [type, setType] = useState<'register' | 'safe' | 'petty_cash' | 'other'>('safe')
  const [stationId, setStationId] = useState('')
  const [openingId, setOpeningId] = useState<string | null>(null)
  const [openingAmount, setOpeningAmount] = useState('')
  const [fromId, setFromId] = useState('')
  const [toId, setToId] = useState('')
  const [transferAmount, setTransferAmount] = useState('')
  const [transferNote, setTransferNote] = useState('')
  const [reconciliationReason, setReconciliationReason] = useState('')
  const [history, setHistory] = useState<{ locationId: string; events: HostedCashLocationEvent[] } | null>(null)
  const [createAttempt, setCreateAttempt] = useState<{ fingerprint: string; key: string } | null>(null)
  const [openingAttempt, setOpeningAttempt] = useState<{ fingerprint: string; key: string } | null>(null)
  const [transferAttempt, setTransferAttempt] = useState<{ fingerprint: string; key: string } | null>(null)
  const [reconciliationAttempt, setReconciliationAttempt] = useState<{ fingerprint: string; key: string } | null>(null)

  const refresh = useCallback(async () => {
    setLoading(true)
    try {
      const [locations, status] = await Promise.all([
        getHostedCashLocations({ apiBaseUrl, accessToken, tenantId, branchId }),
        getHostedCashReconciliationStatus({ apiBaseUrl, accessToken, tenantId, branchId }),
      ])
      setWorkspace(locations)
      setReconciliation(status)
    } catch (reason) {
      setNotice({ kind: 'error', text: reason instanceof Error ? reason.message : 'Cash locations could not be loaded.' })
    } finally { setLoading(false) }
  }, [accessToken, apiBaseUrl, branchId, tenantId])

  useEffect(() => { void refresh() }, [refresh])

  const locations = workspace?.locations ?? []
  const fundLocations = locations.filter((location) => location.status === 'active' && location.initialized && location.locationType !== 'register')

  async function addLocation(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!name.trim()) return
    setBusy(true); setNotice(null)
    try {
      if (type === 'register' && !stationId) { setNotice({ kind: 'error', text: 'Choose the station that owns this register.' }); return }
      const location = { name: name.trim(), locationType: type, ...(type === 'register' ? { stationId } : {}) }
      const fingerprint = JSON.stringify(location)
      const attempt = createAttempt?.fingerprint === fingerprint ? createAttempt : { fingerprint, key: crypto.randomUUID() }
      setCreateAttempt(attempt)
      const result = await createHostedCashLocation({ apiBaseUrl, accessToken, tenantId, branchId, idempotencyKey: attempt.key, location })
      setName(''); await refresh()
      setCreateAttempt(null)
      setNotice({ kind: 'success', text: result.replayed ? 'The saved location was recovered.' : 'Cash location created. Record its physical opening count before moving cash.' })
    } catch (reason) { setNotice({ kind: 'error', text: reason instanceof Error ? reason.message : 'The location could not be created.' }) }
    finally { setBusy(false) }
  }

  async function recordOpening(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!openingId) return
    const amountMinor = minorAmount(openingAmount, true)
    if (amountMinor === null) { setNotice({ kind: 'error', text: 'Enter a valid opening count.' }); return }
    setBusy(true); setNotice(null)
    try {
      const fingerprint = JSON.stringify({ locationId: openingId, amountMinor })
      const attempt = openingAttempt?.fingerprint === fingerprint ? openingAttempt : { fingerprint, key: crypto.randomUUID() }
      setOpeningAttempt(attempt)
      const result = await openHostedCashLocation({ apiBaseUrl, accessToken, tenantId, branchId, locationId: openingId, idempotencyKey: attempt.key, amountMinor })
      setOpeningId(null); setOpeningAmount(''); await refresh()
      setOpeningAttempt(null)
      setNotice({ kind: 'success', text: result.replayed ? 'The opening count was recovered.' : 'Opening count recorded.' })
    } catch (reason) { setNotice({ kind: 'error', text: reason instanceof Error ? reason.message : 'The opening count could not be recorded.' }) }
    finally { setBusy(false) }
  }

  async function makeTransfer(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const amountMinor = minorAmount(transferAmount)
    if (!fromId || !toId || fromId === toId || amountMinor === null || !transferNote.trim()) {
      setNotice({ kind: 'error', text: 'Choose two different initialized locations and enter a valid amount and reason.' }); return
    }
    setBusy(true); setNotice(null)
    try {
      const transfer = { fromLocationId: fromId, toLocationId: toId, amountMinor, note: transferNote.trim() }
      const fingerprint = JSON.stringify(transfer)
      const attempt = transferAttempt?.fingerprint === fingerprint ? transferAttempt : { fingerprint, key: crypto.randomUUID() }
      setTransferAttempt(attempt)
      const result = await transferHostedCash({ apiBaseUrl, accessToken, tenantId, branchId, idempotencyKey: attempt.key, ...transfer })
      setTransferAmount(''); setTransferNote(''); await refresh()
      setTransferAttempt(null)
      setNotice({ kind: 'success', text: result.replayed ? 'The saved transfer was recovered.' : 'Cash transfer recorded.' })
    } catch (reason) { setNotice({ kind: 'error', text: reason instanceof Error ? reason.message : 'The transfer could not be recorded.' }) }
    finally { setBusy(false) }
  }

  async function showHistory(location: HostedCashLocation) {
    setHistory(null); setNotice(null)
    try {
      const events = await getHostedCashLocationEvents({ apiBaseUrl, accessToken, tenantId, branchId, locationId: location.id })
      setHistory({ locationId: location.id, events })
    } catch (reason) { setNotice({ kind: 'error', text: reason instanceof Error ? reason.message : 'Cash history could not be loaded.' }) }
  }

  async function reconcileCash(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const reason = reconciliationReason.trim()
    if (reason.length < 3 || reason.length > 200) { setNotice({ kind: 'error', text: 'Enter a reconciliation reason between 3 and 200 characters.' }); return }
    setBusy(true); setNotice(null)
    try {
      const fingerprint = JSON.stringify({ reason })
      const attempt = reconciliationAttempt?.fingerprint === fingerprint ? reconciliationAttempt : { fingerprint, key: crypto.randomUUID() }
      setReconciliationAttempt(attempt)
      const result = await postHostedCashReconciliation({ apiBaseUrl, accessToken, tenantId, branchId, idempotencyKey: attempt.key, reason })
      setReconciliationReason(''); setReconciliationAttempt(null)
      await refresh()
      setNotice({ kind: 'success', text: result.replayed ? 'The saved cash reconciliation was recovered.' : 'Cash cutover recorded. Any adjustment was posted to opening equity.' })
    } catch (reason) { setNotice({ kind: 'error', text: reason instanceof Error ? reason.message : 'Cash reconciliation could not be completed.' }) }
    finally { setBusy(false) }
  }

  const reconciliationLabel = reconciliation ? ({
    cutover_required: 'Initial cash cutover required', incomplete_locations: 'Opening counts required',
    open_shifts: 'Close all drawer shifts first', unresolved_drawer_movements: 'Review unclassified till movements',
    reconciled: 'Reconciled', variance: 'Cash variance requires review',
  } as const)[reconciliation.status] : 'Reconciliation status unavailable'

  return <section className="hosted-customers-panel cash-locations-panel" aria-labelledby="cash-locations-title">
    <header className="hosted-customers-header"><div><p className="eyebrow">BRANCH FINANCE</p><h2 id="cash-locations-title">Cash locations</h2><p>{branchName} · {locations[0]?.currencyCode ?? 'Branch currency'}</p></div><span className="customer-directory-icon"><Banknote size={18}/></span></header>
    <p className="notice">Cash cutover records the measured branch cash total and posts only the difference to opening equity. Register paid-in and paid-out movements remain review items until they are classified.</p>
    {notice && <p className={`notice ${notice.kind === 'success' ? 'notice-success' : 'notice-danger'}`} role={notice.kind === 'error' ? 'alert' : 'status'}>{notice.text}</p>}
    {reconciliation && <section className="cash-reconciliation" aria-label="Cash to ledger reconciliation">
      <header><div><h3>Cash to ledger</h3><p>{reconciliationLabel}</p></div><strong>{money(reconciliation.currencyCode, reconciliation.varianceMinor)} variance</strong></header>
      <dl><div><dt>Tracked physical cash</dt><dd>{money(reconciliation.currencyCode,reconciliation.trackedCashMinor)}</dd></div><div><dt>Cash account balance</dt><dd>{money(reconciliation.currencyCode,reconciliation.ledgerCashMinor)}</dd></div><div><dt>Active cash locations</dt><dd>{reconciliation.activeLocationCount}</dd></div><div><dt>Uninitialized locations</dt><dd>{reconciliation.uninitializedLocationCount}</dd></div><div><dt>Open drawer shifts</dt><dd>{reconciliation.openShiftCount}</dd></div><div><dt>Unclassified drawer movements</dt><dd>{reconciliation.unresolvedDrawerMovementCount}</dd></div></dl>
      {reconciliation.latest && <p>Last cutover {new Date(reconciliation.latest.createdAt).toLocaleString()} · adjustment {money(reconciliation.currencyCode,reconciliation.latest.adjustmentMinor)} · {reconciliation.latest.reason}</p>}
      {canReconcile && <form className="cash-location-form" onSubmit={(event) => void reconcileCash(event)}>
        <h3>Record or refresh cash cutover</h3><label>Reason<input maxLength={200} minLength={3} required value={reconciliationReason} onChange={(event) => setReconciliationReason(event.currentTarget.value)} disabled={busy}/></label>
        <p>Requires all active locations to have opening counts and every drawer shift to be closed. A nonzero adjustment is posted to account 3000 (opening equity) with this reason.</p>
        <button className="button button-primary" type="submit" disabled={busy || reconciliation.activeLocationCount === 0 || reconciliation.uninitializedLocationCount > 0 || reconciliation.openShiftCount > 0}>{busy ? 'Reconciling…' : 'Reconcile cash'}</button>
      </form>}
    </section>}
    {canManage && <div className="cash-location-forms">
      <form className="cash-location-form" onSubmit={(event) => void addLocation(event)}>
        <h3><Plus size={15}/> Add cash location</h3>
        <label>Location name<input maxLength={100} required value={name} onChange={(event) => setName(event.currentTarget.value)} disabled={busy}/></label>
        <label>Type<select value={type} onChange={(event) => { setType(event.currentTarget.value as typeof type); setStationId('') }} disabled={busy}><option value="safe">Safe</option><option value="petty_cash">Petty cash</option><option value="other">Other cash</option><option value="register">Station register</option></select></label>
        {type === 'register' && <label>Station<select required value={stationId} onChange={(event) => setStationId(event.currentTarget.value)} disabled={busy}><option value="">Select station</option>{stations.filter((station) => !locations.some((location) => location.locationType === 'register' && location.stationId === station.stationId)).map((station) => <option key={station.stationId} value={station.stationId}>{station.name} · {station.code}</option>)}</select></label>}
        <p>{type === 'register' ? 'Record a physical cash count while this station’s drawer is closed. It starts the location ledger at cutover.' : 'Register drawers are station-specific and cannot be moved through this branch transfer form.'}</p>
        <button className="button button-primary" type="submit" disabled={busy}>{busy ? 'Saving…' : 'Create location'}</button>
      </form>
      {openingId ? <form className="cash-location-form" onSubmit={(event) => void recordOpening(event)}>
        <h3><Banknote size={15}/> Physical opening count</h3>
        <label>Counted cash<input inputMode="decimal" required value={openingAmount} onChange={(event) => setOpeningAmount(event.currentTarget.value)} disabled={busy}/></label>
        <p>This one-time count starts tracked history and does not post another GL opening journal.</p>
        <div><button className="button button-primary" disabled={busy}>{busy ? 'Saving…' : 'Record opening'}</button><button className="button button-quiet" type="button" onClick={() => setOpeningId(null)} disabled={busy}>Cancel</button></div>
      </form> : <form className="cash-location-form" onSubmit={(event) => void makeTransfer(event)}>
        <h3><ArrowLeftRight size={15}/> Transfer cash</h3>
        <label>From<select required value={fromId} onChange={(event) => setFromId(event.currentTarget.value)} disabled={busy}><option value="">Select source</option>{fundLocations.map((location) => <option key={location.id} value={location.id}>{location.name} · {money(location.currencyCode,location.balanceMinor)}</option>)}</select></label>
        <label>To<select required value={toId} onChange={(event) => setToId(event.currentTarget.value)} disabled={busy}><option value="">Select destination</option>{fundLocations.map((location) => <option key={location.id} value={location.id}>{location.name}</option>)}</select></label>
        <label>Amount<input inputMode="decimal" required value={transferAmount} onChange={(event) => setTransferAmount(event.currentTarget.value)} disabled={busy}/></label>
        <label>Reason<input maxLength={240} required value={transferNote} onChange={(event) => setTransferNote(event.currentTarget.value)} disabled={busy}/></label>
        <button className="button button-primary" type="submit" disabled={busy || fundLocations.length < 2}>{busy ? 'Saving…' : 'Record transfer'}</button>
      </form>}
    </div>}
    <div className="hosted-customer-table-wrap"><table className="hosted-customer-table"><thead><tr><th>Location</th><th>Type</th><th>Tracked balance</th><th>Status</th><th>Actions</th></tr></thead><tbody>{locations.map((location) => <tr key={location.id}><td><strong>{location.name}</strong>{location.stationId && <small>Station register</small>}</td><td>{location.locationType.replaceAll('_',' ')}</td><td><strong>{money(location.currencyCode,location.balanceMinor)}</strong></td><td>{location.initialized ? 'Tracked' : 'Opening count required'}</td><td><button className="button button-quiet" type="button" onClick={() => void showHistory(location)}><History size={14}/> History</button>{canManage && !location.initialized && <button className="button button-quiet" type="button" onClick={() => { setOpeningId(location.id); setOpeningAmount('') }}>Set opening</button>}</td></tr>)}</tbody></table>{loading && <p className="hosted-customer-empty" role="status">Loading cash locations…</p>}{!loading && locations.length===0 && <p className="hosted-customer-empty">No cash locations set up for this branch.</p>}</div>
    {history && <section className="cash-history" aria-label="Cash location history"><header><div><h3>Movement history</h3><p>{locations.find((location)=>location.id===history.locationId)?.name}</p></div><button className="button button-quiet" type="button" onClick={()=>setHistory(null)}>Close</button></header>{history.events.length===0 ? <p>No cash events recorded yet.</p> : <ul>{history.events.map((event)=><li key={event.id}><span><strong>{eventLabel(event)}</strong><small>{new Date(event.createdAt).toLocaleString()} · {event.actorName}</small></span><strong>{money(locations.find((location)=>location.id===history.locationId)?.currencyCode??'USD',event.signedAmountMinor)}</strong></li>)}</ul>}</section>}
    <footer className="hosted-customer-pagination"><span>{locations.length} location{locations.length===1?'':'s'}</span><button type="button" className="button button-quiet" onClick={()=>void refresh()} disabled={loading}><RefreshCw size={13}/> Refresh</button></footer>
  </section>
}
