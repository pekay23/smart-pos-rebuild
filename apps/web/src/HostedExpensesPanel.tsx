// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useEffect, useState } from 'react'
import type { FormEvent } from 'react'
import { ReceiptText, RefreshCw } from 'lucide-react'
import { createBranchExpense, createBranchExpenseReimbursement, getBranchExpenseCostCenterHistory, getBranchExpenseReimbursements, getBranchExpenses, getHostedCashLocations, reclassifyBranchExpense } from './api'
import type { HostedCashLocation, HostedCostCenterKind, HostedExpense, HostedExpenseCategory, HostedExpenseCostCenterHistory, HostedExpenseInput, HostedExpensePaymentMethod, HostedExpenseReimbursement, HostedExpenseReimbursementWorkspace, HostedExpenseWorkspace, StationAccess } from './api'

const PAGE_SIZE = 50
const categories: ReadonlyArray<{ value: HostedExpenseCategory; label: string }> = [
  { value: 'rent', label: 'Rent' }, { value: 'utilities', label: 'Utilities' }, { value: 'wages', label: 'Wages' },
  { value: 'transport', label: 'Transport' }, { value: 'supplies', label: 'Supplies' }, { value: 'maintenance', label: 'Maintenance' },
  { value: 'marketing', label: 'Marketing' }, { value: 'bank_fees', label: 'Bank fees' }, { value: 'taxes_fees', label: 'Taxes and fees' }, { value: 'other', label: 'Other' },
]
const paymentMethods: ReadonlyArray<{ value: HostedExpensePaymentMethod; label: string }> = [
  { value: 'bank_transfer', label: 'Bank transfer' }, { value: 'mobile_money', label: 'Mobile money' }, { value: 'card', label: 'Card' }, { value: 'other', label: 'Other non-cash' },
]
const expensePaymentMethods: ReadonlyArray<{ value: HostedExpense['paymentMethod']; label: string }> = [{ value: 'cash', label: 'Safe / petty cash' }, ...paymentMethods]

function categoryValue(value: string): HostedExpenseCategory {
  return categories.find((category) => category.value === value)?.value ?? 'other'
}

function paymentMethodValue(value: string): HostedExpense['paymentMethod'] {
  return expensePaymentMethods.find((method) => method.value === value)?.value ?? 'other'
}

function localDate(): string {
  const now = new Date()
  return `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}-${String(now.getDate()).padStart(2, '0')}`
}

function parseMinor(value: string): number | null {
  const trimmed = value.trim()
  if (!/^\d+(?:\.\d{1,2})?$/.test(trimmed)) return null
  const [whole = '', fraction = ''] = trimmed.split('.')
  const amount = Number(whole) * 100 + Number(fraction.padEnd(2, '0'))
  return Number.isSafeInteger(amount) && amount > 0 ? amount : null
}

function formatMoney(currency: string, minor: number): string {
  return new Intl.NumberFormat(undefined, { style: 'currency', currency }).format(minor / 100)
}

function formatCategory(value: HostedExpenseCategory): string {
  return categories.find((category) => category.value === value)?.label ?? value
}

export function HostedExpensesPanel({ apiBaseUrl, accessToken, tenantId, branchId, branchName, stations, canWrite, canReclassify }: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; branchName: string; stations: StationAccess[]; canWrite: boolean; canReclassify: boolean
}) {
  const [workspace, setWorkspace] = useState<HostedExpenseWorkspace | null>(null)
  const [cashLocations, setCashLocations] = useState<HostedCashLocation[]>([])
  const [offset, setOffset] = useState(0)
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState(false)
  const [formOpen, setFormOpen] = useState(false)
  const [description, setDescription] = useState('')
  const [categoryCode, setCategoryCode] = useState<HostedExpenseCategory>('other')
  const [payee, setPayee] = useState('')
  const [amount, setAmount] = useState('')
  const [paymentMethod, setPaymentMethod] = useState<HostedExpense['paymentMethod']>('bank_transfer')
  const [cashLocationId, setCashLocationId] = useState('')
  const [externalReference, setExternalReference] = useState('')
  const [occurredOn, setOccurredOn] = useState(localDate)
  const [attempt, setAttempt] = useState<{ fingerprint: string; key: string } | null>(null)
  const [notice, setNotice] = useState<{ kind: 'success' | 'error'; text: string } | null>(null)
  const [reimbursementTarget, setReimbursementTarget] = useState<HostedExpense | null>(null)
  const [reimbursementWorkspace, setReimbursementWorkspace] = useState<HostedExpenseReimbursementWorkspace | null>(null)
  const [reimbursementAmount, setReimbursementAmount] = useState('')
  const [reimbursementMethod, setReimbursementMethod] = useState<HostedExpenseReimbursement['returnMethod']>('cash')
  const [reimbursementLocation, setReimbursementLocation] = useState('')
  const [reimbursementReference, setReimbursementReference] = useState('')
  const [reimbursementReason, setReimbursementReason] = useState('')
  const [reimbursementDate, setReimbursementDate] = useState(localDate)
  const [reimbursementAttempt, setReimbursementAttempt] = useState<{ fingerprint: string; key: string } | null>(null)
  const [attributionTarget, setAttributionTarget] = useState<HostedExpense | null>(null)
  const [attributionHistory, setAttributionHistory] = useState<HostedExpenseCostCenterHistory | null>(null)
  const [attributionKind, setAttributionKind] = useState<HostedCostCenterKind>('legacy_unassigned')
  const [attributionStationId, setAttributionStationId] = useState('')
  const [attributionReason, setAttributionReason] = useState('')
  const [attributionAttempt, setAttributionAttempt] = useState<{ fingerprint: string; key: string } | null>(null)

  useEffect(() => {
    let current = true
    setLoading(true)
    void getBranchExpenses({ apiBaseUrl, accessToken, tenantId, branchId, offset, pageSize: PAGE_SIZE })
      .then((result) => { if (current) setWorkspace(result) })
      .catch((reason: unknown) => { if (current) setNotice({ kind: 'error', text: reason instanceof Error ? reason.message : 'Expenses could not be loaded.' }) })
      .finally(() => { if (current) setLoading(false) })
    return () => { current = false }
  }, [accessToken, apiBaseUrl, branchId, offset, tenantId])

  useEffect(() => {
    let current = true
    void getHostedCashLocations({ apiBaseUrl, accessToken, tenantId, branchId })
      .then((result) => { if (current) setCashLocations(result.locations.filter((location) => location.status === 'active' && location.locationType !== 'register' && location.initialized)) })
      .catch(() => { if (current) setCashLocations([]) })
    return () => { current = false }
  }, [accessToken, apiBaseUrl, branchId, tenantId])

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const amountMinor = parseMinor(amount)
    if (!description.trim() || amountMinor === null || !occurredOn || (paymentMethod === 'cash' && !cashLocationId)) {
      setNotice({ kind: 'error', text: paymentMethod === 'cash' && !cashLocationId ? 'Choose the initialized safe or petty-cash location used for this expense.' : 'Enter a description, valid amount and expense date.' })
      return
    }
    const expense: HostedExpenseInput = {
      categoryCode, description: description.trim(), payee: payee.trim() || null, amountMinor,
      paymentMethod, cashLocationId: paymentMethod === 'cash' ? cashLocationId : null, externalReference: externalReference.trim() || null, occurredOn,
    }
    const fingerprint = JSON.stringify(expense)
    const request = attempt?.fingerprint === fingerprint ? attempt : { fingerprint, key: crypto.randomUUID() }
    setAttempt(request)
    setBusy(true)
    setNotice(null)
    try {
      const result = await createBranchExpense({ apiBaseUrl, accessToken, tenantId, branchId, idempotencyKey: request.key, expense })
      setOffset(0)
      setWorkspace((current) => current === null ? current : { ...current, expenses: [result.expense, ...current.expenses].slice(0, PAGE_SIZE), offset: 0 })
      setDescription('')
      setPayee('')
      setAmount('')
      setExternalReference('')
      setCashLocationId('')
      setOccurredOn(localDate())
      setAttempt(null)
      setFormOpen(false)
      setNotice({ kind: 'success', text: result.replayed ? 'The saved expense was recovered after a retry.' : 'Expense recorded and journaled.' })
    } catch (reason) {
      setNotice({ kind: 'error', text: reason instanceof Error ? reason.message : 'The expense could not be recorded. Retry the same details safely.' })
    } finally { setBusy(false) }
  }

  async function openReimbursement(expense: HostedExpense) {
    setReimbursementTarget(expense)
    setReimbursementWorkspace(null)
    setReimbursementAmount('')
    setReimbursementReference('')
    setReimbursementReason('')
    setReimbursementLocation('')
    setReimbursementDate(localDate())
    setNotice(null)
    setBusy(true)
    try {
      setReimbursementWorkspace(await getBranchExpenseReimbursements({ apiBaseUrl, accessToken, tenantId, branchId, expenseId: expense.id }))
    } catch (reason) {
      setNotice({ kind: 'error', text: reason instanceof Error ? reason.message : 'Reimbursement history could not be loaded.' })
    } finally { setBusy(false) }
  }

  async function submitReimbursement(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!reimbursementTarget) return
    const amountMinor = parseMinor(reimbursementAmount)
    const needsCashLocation = reimbursementMethod === 'cash'
    if (amountMinor === null || !reimbursementDate || !reimbursementReason.trim() || (needsCashLocation && !reimbursementLocation) || (!needsCashLocation && !reimbursementReference.trim())) {
      setNotice({ kind: 'error', text: needsCashLocation ? 'Enter the amount and reason, and choose the cash location that received the return.' : 'Enter the amount, reason and external payment reference.' })
      return
    }
    const reimbursement = {
      amountMinor, returnMethod: reimbursementMethod, cashLocationId: needsCashLocation ? reimbursementLocation : null,
      externalReference: needsCashLocation ? null : reimbursementReference.trim(), reason: reimbursementReason.trim(), returnedOn: reimbursementDate,
    }
    const fingerprint = JSON.stringify({ expenseId: reimbursementTarget.id, ...reimbursement })
    const request = reimbursementAttempt?.fingerprint === fingerprint ? reimbursementAttempt : { fingerprint, key: crypto.randomUUID() }
    setReimbursementAttempt(request)
    setBusy(true)
    setNotice(null)
    try {
      const result = await createBranchExpenseReimbursement({ apiBaseUrl, accessToken, tenantId, branchId, expenseId: reimbursementTarget.id, idempotencyKey: request.key, reimbursement })
      const next = await getBranchExpenseReimbursements({ apiBaseUrl, accessToken, tenantId, branchId, expenseId: reimbursementTarget.id })
      setReimbursementWorkspace(next)
      setReimbursementTarget((current) => current ? { ...current, reimbursedMinor: next.reimbursedMinor, remainingReimbursableMinor: next.remainingMinor } : current)
      setWorkspace((current) => current ? { ...current, expenses: current.expenses.map((expense) => expense.id === reimbursementTarget.id ? { ...expense, reimbursedMinor: next.reimbursedMinor, remainingReimbursableMinor: next.remainingMinor } : expense) } : current)
      setReimbursementAmount('')
      setReimbursementReference('')
      setReimbursementReason('')
      setReimbursementLocation('')
      setReimbursementAttempt(null)
      setNotice({ kind: 'success', text: result.replayed ? 'The saved reimbursement was recovered.' : 'Expense return recorded and journaled.' })
    } catch (reason) {
      setNotice({ kind: 'error', text: reason instanceof Error ? reason.message : 'The reimbursement could not be recorded. Retry unchanged to avoid duplicates.' })
    } finally { setBusy(false) }
  }

  async function openAttributionCorrection(expense: HostedExpense) {
    setAttributionTarget(expense)
    setAttributionHistory(null)
    setAttributionKind(expense.costCenterKind)
    setAttributionStationId(expense.costCenterStationId ?? '')
    setAttributionReason('')
    setAttributionAttempt(null)
    setNotice(null)
    setBusy(true)
    try {
      const history = await getBranchExpenseCostCenterHistory({ apiBaseUrl, accessToken, tenantId, branchId, expenseId: expense.id })
      setAttributionHistory(history)
      setAttributionKind(history.currentKind)
      setAttributionStationId(history.currentStationId ?? '')
    } catch (reason) {
      setAttributionTarget(null)
      setNotice({ kind: 'error', text: reason instanceof Error ? reason.message : 'Expense attribution history could not be loaded.' })
    } finally { setBusy(false) }
  }

  async function submitAttributionCorrection(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!attributionTarget || !attributionHistory) return
    const stationId = attributionKind === 'station_direct' ? attributionStationId : null
    if ((attributionKind === 'station_direct' && !stationId) || attributionReason.trim().length < 5) {
      setNotice({ kind: 'error', text: 'Choose an active station for direct costs and explain the correction in at least 5 characters.' })
      return
    }
    const correction = { expectedRevision: attributionHistory.currentRevision, costCenterKind: attributionKind, stationId, reason: attributionReason.trim() }
    const fingerprint = JSON.stringify({ expenseId: attributionTarget.id, correction })
    const request = attributionAttempt?.fingerprint === fingerprint ? attributionAttempt : { fingerprint, key: crypto.randomUUID() }
    setAttributionAttempt(request)
    setBusy(true)
    setNotice(null)
    try {
      await reclassifyBranchExpense({ apiBaseUrl, accessToken, tenantId, branchId, expenseId: attributionTarget.id, idempotencyKey: request.key, correction })
      const [history, page] = await Promise.all([
        getBranchExpenseCostCenterHistory({ apiBaseUrl, accessToken, tenantId, branchId, expenseId: attributionTarget.id }),
        getBranchExpenses({ apiBaseUrl, accessToken, tenantId, branchId, offset, pageSize: PAGE_SIZE }),
      ])
      setAttributionHistory(history)
      setAttributionKind(history.currentKind)
      setAttributionStationId(history.currentStationId ?? '')
      setAttributionReason('')
      setAttributionAttempt(null)
      setWorkspace(page)
      setNotice({ kind: 'success', text: 'Expense attribution corrected. The original expense and any returns use this cost center in reports.' })
    } catch (reason) {
      setNotice({ kind: 'error', text: reason instanceof Error ? reason.message : 'The attribution correction could not be saved. Retry the same details safely.' })
    } finally { setBusy(false) }
  }

  function costCenterLabel(kind: HostedCostCenterKind, stationName: string | null) {
    if (kind === 'station_direct') return stationName || 'Register station'
    if (kind === 'store_overhead') return 'Store overhead'
    return 'Not allocated'
  }

  function renderRow(row: HostedExpense) {
    return <tr key={row.id}><td><strong>{formatCategory(row.categoryCode)}</strong><small>{row.description}{row.payee ? ` · ${row.payee}` : ''}</small></td><td>{row.occurredOn}</td><td>{row.paymentMethod.replaceAll('_', ' ')}{row.cashLocationId && <small>{cashLocations.find((location) => location.id === row.cashLocationId)?.name ?? 'Cash location'}</small>}{row.externalReference && <small>{row.externalReference}</small>}</td><td><strong>{costCenterLabel(row.costCenterKind, row.costCenterStationNameSnapshot)}</strong>{row.costCenterRevision > 0 && <small>Correction {row.costCenterRevision}</small>}</td><td>{row.recordedBy}</td><td><strong>{formatMoney(row.currencyCode, row.amountMinor)}</strong>{row.reimbursedMinor > 0 && <small>Returned {formatMoney(row.currencyCode, row.reimbursedMinor)}</small>}</td><td><div className="hosted-expense-actions">{canWrite && row.remainingReimbursableMinor > 0 && <button type="button" className="button button-quiet" onClick={() => void openReimbursement(row)} disabled={busy}>Record return</button>}{canReclassify && <button type="button" className="button button-quiet" onClick={() => void openAttributionCorrection(row)} disabled={busy}>Correct cost center</button>}{!canWrite && !canReclassify ? '—' : null}</div></td></tr>
  }

  return <section className="hosted-customers-panel" aria-labelledby="hosted-expenses-title">
    <header className="hosted-customers-header"><div><p className="eyebrow">BRANCH FINANCE</p><h2 id="hosted-expenses-title">Expenses</h2><p>{branchName} · {workspace?.currencyCode ?? 'Branch currency'}</p></div><span className="customer-directory-icon"><ReceiptText size={18}/></span></header>
    <div className="hosted-customer-create-toggle"><span>Expenses are recorded as immutable transactions with a balanced journal entry.</span>{canWrite && <button className="button button-primary" type="button" onClick={() => setFormOpen((open) => !open)} disabled={busy}>{formOpen ? 'Cancel' : 'Record expense'}</button>}</div>
    <p className="notice">Cash expenses are paid from an initialized safe or petty-cash location. Register cash must be recorded through the signed cashier drawer workflow. External payment references are reported by your team and are not provider-verified.</p>
    {formOpen && <form className="hosted-customer-edit" onSubmit={(event) => void submit(event)} aria-label="Record a branch expense">
      <div className="hosted-customer-edit-heading"><div><p className="eyebrow">NEW TRANSACTION</p><h3>Record expense</h3></div></div>
      <div className="hosted-customer-fields">
        <label>Category<select value={categoryCode} onChange={(event) => setCategoryCode(categoryValue(event.currentTarget.value))} disabled={busy}>{categories.map((item) => <option key={item.value} value={item.value}>{item.label}</option>)}</select></label>
        <label>Amount ({workspace?.currencyCode ?? 'currency'})<input required inputMode="decimal" value={amount} onChange={(event) => setAmount(event.currentTarget.value)} placeholder="0.00" disabled={busy}/></label>
        <label>Description<input required maxLength={240} value={description} onChange={(event) => setDescription(event.currentTarget.value)} disabled={busy}/></label>
        <label>Payee / supplier<input maxLength={160} value={payee} onChange={(event) => setPayee(event.currentTarget.value)} disabled={busy}/></label>
        <label>Payment method<select value={paymentMethod} onChange={(event) => setPaymentMethod(paymentMethodValue(event.currentTarget.value))} disabled={busy}>{expensePaymentMethods.map((item) => <option key={item.value} value={item.value}>{item.label}</option>)}</select></label>
        {paymentMethod === 'cash' && <label>Paid from<select required value={cashLocationId} onChange={(event) => setCashLocationId(event.currentTarget.value)} disabled={busy}><option value="">Choose initialized safe / petty cash</option>{cashLocations.map((location) => <option key={location.id} value={location.id}>{location.name} · {formatMoney(location.currencyCode, location.balanceMinor)}</option>)}</select></label>}
        <label>Payment reference<input maxLength={160} value={externalReference} onChange={(event) => setExternalReference(event.currentTarget.value)} disabled={busy}/></label>
        <label>Date incurred<input required type="date" value={occurredOn} onChange={(event) => setOccurredOn(event.currentTarget.value)} disabled={busy}/></label>
      </div>
      <button type="submit" className="button button-primary" disabled={busy}>{busy ? 'Saving…' : 'Save expense'}</button>
    </form>}
    {reimbursementTarget && <form className="hosted-customer-edit" onSubmit={(event) => void submitReimbursement(event)} aria-label="Record an expense return">
      <div className="hosted-customer-edit-heading"><div><p className="eyebrow">EXPENSE CORRECTION</p><h3>Record return · {reimbursementTarget.description}</h3><p>{reimbursementWorkspace ? `${formatMoney(reimbursementTarget.currencyCode, reimbursementWorkspace.reimbursedMinor)} returned · ${formatMoney(reimbursementTarget.currencyCode, reimbursementWorkspace.remainingMinor)} remaining` : 'Loading reimbursement history…'}</p></div><button className="button button-quiet" type="button" disabled={busy} onClick={() => { setReimbursementTarget(null); setReimbursementWorkspace(null); setReimbursementAttempt(null) }}>Close</button></div>
      {reimbursementWorkspace?.reimbursements.length ? <ul className="hosted-customer-list">{reimbursementWorkspace.reimbursements.map((item) => <li key={item.id}><strong>{formatMoney(reimbursementTarget.currencyCode, item.amountMinor)} · {item.returnMethod.replaceAll('_', ' ')}</strong><small>{item.returnedOn} · {item.recordedBy} · {item.reason}{item.externalReference ? ` · Ref ${item.externalReference}` : ''}</small></li>)}</ul> : null}
      {reimbursementWorkspace && reimbursementWorkspace.remainingMinor > 0 && <><div className="hosted-customer-fields"><label>Return amount<input required inputMode="decimal" value={reimbursementAmount} onChange={(event) => setReimbursementAmount(event.currentTarget.value)} placeholder="0.00" disabled={busy}/></label><label>Return method<select value={reimbursementMethod} onChange={(event) => setReimbursementMethod(event.currentTarget.value as HostedExpenseReimbursement['returnMethod'])} disabled={busy}><option value="cash">Cash received</option><option value="bank_transfer">Bank transfer</option><option value="mobile_money">Mobile money</option><option value="card">Card reversal</option><option value="other">Other non-cash</option></select></label>{reimbursementMethod === 'cash' ? <label>Cash received into<select required value={reimbursementLocation} onChange={(event) => setReimbursementLocation(event.currentTarget.value)} disabled={busy}><option value="">Choose initialized safe / petty cash</option>{cashLocations.map((location) => <option key={location.id} value={location.id}>{location.name}</option>)}</select></label> : <label>External return reference<input required maxLength={160} value={reimbursementReference} onChange={(event) => setReimbursementReference(event.currentTarget.value)} disabled={busy}/></label>}<label>Reason<textarea required minLength={5} maxLength={500} value={reimbursementReason} onChange={(event) => setReimbursementReason(event.currentTarget.value)} disabled={busy}/></label><label>Date received<input required type="date" value={reimbursementDate} onChange={(event) => setReimbursementDate(event.currentTarget.value)} disabled={busy}/></label></div><button type="submit" className="button button-primary" disabled={busy || !reimbursementWorkspace}>{busy ? 'Saving…' : 'Record return'}</button></>}
    </form>}
    {attributionTarget && <form className="hosted-customer-edit" onSubmit={(event) => void submitAttributionCorrection(event)} aria-label="Correct expense cost center">
      <div className="hosted-customer-edit-heading"><div><p className="eyebrow">REPORTING CORRECTION</p><h3>Cost center · {attributionTarget.description}</h3><p>Changes affect this expense and its returns on their original dates. The journal and cash history stay unchanged.</p></div><button className="button button-quiet" type="button" disabled={busy} onClick={() => { setAttributionTarget(null); setAttributionHistory(null); setAttributionAttempt(null) }}>Close</button></div>
      {attributionHistory?.rows.length ? <ul className="hosted-customer-list">{attributionHistory.rows.map((item) => <li key={item.id}><strong>{costCenterLabel(item.previousKind, item.previousStationNameSnapshot)} → {costCenterLabel(item.nextKind, item.nextStationNameSnapshot)}</strong><small>Revision {item.revision} · {item.recordedAt} · {item.actorName} · {item.reason}</small></li>)}</ul> : null}
      <div className="hosted-customer-fields"><label>Cost center<select value={attributionKind} onChange={(event) => { setAttributionKind(event.currentTarget.value as HostedCostCenterKind); setAttributionStationId('') }} disabled={busy || !attributionHistory}><option value="station_direct">Register station</option><option value="store_overhead">Store overhead</option><option value="legacy_unassigned">Not allocated</option></select></label>{attributionKind === 'station_direct' && <label>Register station<select required value={attributionStationId} onChange={(event) => setAttributionStationId(event.currentTarget.value)} disabled={busy || !attributionHistory}><option value="">Choose station</option>{stations.map((station) => <option key={station.stationId} value={station.stationId}>{station.name}</option>)}</select></label>}<label>Correction reason<textarea required minLength={5} maxLength={500} value={attributionReason} onChange={(event) => setAttributionReason(event.currentTarget.value)} disabled={busy || !attributionHistory}/></label></div>
      <button type="submit" className="button button-primary" disabled={busy || !attributionHistory || (attributionKind === attributionHistory.currentKind && attributionStationId === (attributionHistory.currentStationId ?? ''))}>{busy ? 'Saving…' : 'Save correction'}</button>
    </form>}
    {notice && <p className={`notice ${notice.kind === 'success' ? 'notice-success' : 'notice-danger'}`} role={notice.kind === 'error' ? 'alert' : 'status'}>{notice.text}</p>}
    <div className="hosted-customer-table-wrap"><table className="hosted-customer-table"><thead><tr><th>Expense</th><th>Date</th><th>Payment</th><th>Cost center</th><th>Recorded by</th><th>Amount</th><th>Actions</th></tr></thead><tbody>{workspace?.expenses.map(renderRow)}</tbody></table>{!loading && workspace?.expenses.length === 0 && <p className="hosted-customer-empty">No expenses recorded for this branch.</p>}{loading && <p className="hosted-customer-empty" role="status">Loading expenses…</p>}</div>
    <footer className="hosted-customer-pagination"><span>{workspace?.expenses.length ? `${offset + 1}–${offset + workspace.expenses.length}` : 'No records'}</span><div><button type="button" className="button button-quiet" onClick={() => setOffset(Math.max(0, offset - PAGE_SIZE))} disabled={offset === 0 || loading}><RefreshCw size={13}/> Previous</button><button type="button" className="button button-quiet" onClick={() => setOffset(offset + PAGE_SIZE)} disabled={!workspace?.hasMore || loading}>Next</button></div></footer>
  </section>
}
