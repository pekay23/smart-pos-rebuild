// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useEffect, useState } from 'react'
import type { FormEvent } from 'react'
import { Archive, Edit3, RefreshCw, RotateCcw, Search, UserPlus, UserRound } from 'lucide-react'
import { createBranchHostedCustomer, listBranchHostedCustomers, listTenantHostedCustomerCreditTermHistory, listTenantHostedCustomerCreditTerms, mergeBranchHostedCustomer, setBranchHostedCustomerStatus, setTenantHostedCustomerCreditTerm, updateBranchHostedCustomer } from './api'
import type { HostedCustomer, HostedCustomerCreditTerm, HostedCustomerCreditTermEvent } from './api'
import { HostedEntityImage } from './HostedEntityImage'

const PAGE_SIZE = 50

export function HostedCustomersPanel({ apiBaseUrl, accessToken, tenantId, branchId, branchName, currencyCode, canManageCreditTerms }: {
  apiBaseUrl: string
  accessToken: string
  tenantId: string
  branchId: string
  branchName: string
  currencyCode: string
  canManageCreditTerms: boolean
}) {
  const [customers, setCustomers] = useState<HostedCustomer[]>([])
  const [query, setQuery] = useState('')
  const [appliedQuery, setAppliedQuery] = useState('')
  const [includeArchived, setIncludeArchived] = useState(false)
  const [offset, setOffset] = useState(0)
  const [loading, setLoading] = useState(true)
  const [busyCustomer, setBusyCustomer] = useState<string | null>(null)
  const [editingCustomer, setEditingCustomer] = useState<HostedCustomer | null>(null)
  const [createFormOpen, setCreateFormOpen] = useState(false)
  const [creatingCustomer, setCreatingCustomer] = useState(false)
  const [createForm, setCreateForm] = useState({ name: '', phone: '', email: '', notes: '' })
  const [createAttempt, setCreateAttempt] = useState<{ fingerprint: string; key: string } | null>(null)
  const [form, setForm] = useState({ name: '', phone: '', email: '', notes: '' })
  const [notice, setNotice] = useState<{ kind: 'success' | 'error'; text: string } | null>(null)
  const [mergeSource, setMergeSource] = useState<HostedCustomer | null>(null)
  const [mergeTargetId, setMergeTargetId] = useState('')
  const [confirmMerge, setConfirmMerge] = useState(false)
  const [mergeAttempt, setMergeAttempt] = useState<{ sourceId: string; targetId: string; key: string } | null>(null)
  const [creditCustomer, setCreditCustomer] = useState<HostedCustomer | null>(null)
  const [creditTerms, setCreditTerms] = useState<HostedCustomerCreditTerm[]>([])
  const [creditTermsLoading, setCreditTermsLoading] = useState(false)
  const [creditHistory, setCreditHistory] = useState<HostedCustomerCreditTermEvent[]>([])
  const [creditHistoryLoading, setCreditHistoryLoading] = useState(false)
  const [creditHistoryError, setCreditHistoryError] = useState<string | null>(null)
  const [creditCurrency, setCreditCurrency] = useState(currencyCode.toUpperCase())
  const [creditLimit, setCreditLimit] = useState('')
  const [creditDueDays, setCreditDueDays] = useState('30')
  const [creditReason, setCreditReason] = useState('')
  const [creditAttempt, setCreditAttempt] = useState<{ fingerprint: string; key: string } | null>(null)
  const [savingCreditTerms, setSavingCreditTerms] = useState(false)

  useEffect(() => {
    let current = true
    setLoading(true)
    void listBranchHostedCustomers({ apiBaseUrl, accessToken, tenantId, branchId, query: appliedQuery, offset, pageSize: PAGE_SIZE, includeArchived })
      .then((rows) => { if (current) setCustomers(rows) })
      .catch((reason: unknown) => { if (current) setNotice({ kind: 'error', text: reason instanceof Error ? reason.message : 'Customer records could not be loaded.' }) })
      .finally(() => { if (current) setLoading(false) })
    return () => { current = false }
  }, [accessToken, apiBaseUrl, appliedQuery, branchId, includeArchived, offset, tenantId])

  useEffect(() => {
    if (!creditCustomer) {
      setCreditHistory([])
      setCreditHistoryError(null)
      return
    }
    let current = true
    setCreditHistoryLoading(true)
    setCreditHistoryError(null)
    void listTenantHostedCustomerCreditTermHistory({
      apiBaseUrl, accessToken, tenantId, customerId: creditCustomer.id, currencyCode: creditCurrency,
    }).then((events) => { if (current) setCreditHistory(events) })
      .catch((reason: unknown) => {
        if (current) setCreditHistoryError(reason instanceof Error ? reason.message : 'Credit term history could not be loaded.')
      })
      .finally(() => { if (current) setCreditHistoryLoading(false) })
    return () => { current = false }
  }, [accessToken, apiBaseUrl, creditCustomer?.id, creditCurrency, tenantId])

  function search(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setOffset(0)
    setAppliedQuery(query.trim())
  }

  async function createCustomer(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const customer = { name: createForm.name.trim(), phone: createForm.phone.trim() || null, email: createForm.email.trim() || null, notes: createForm.notes.trim() || null }
    if (!customer.name) { setNotice({ kind: 'error', text: 'Enter the customer’s name.' }); return }
    const fingerprint = JSON.stringify(customer)
    const attempt = createAttempt?.fingerprint === fingerprint ? createAttempt : { fingerprint, key: crypto.randomUUID() }
    setCreateAttempt(attempt)
    setCreatingCustomer(true)
    setNotice(null)
    try {
      const created = await createBranchHostedCustomer({ apiBaseUrl, accessToken, tenantId, branchId, idempotencyKey: attempt.key, customer })
      setCreateAttempt(null)
      setCreateForm({ name: '', phone: '', email: '', notes: '' })
      setCreateFormOpen(false)
      setQuery('')
      setAppliedQuery('')
      setIncludeArchived(false)
      setOffset(0)
      setCustomers((current) => [created, ...current.filter((row) => row.id !== created.id)].slice(0, PAGE_SIZE))
      setNotice({ kind: 'success', text: `${created.name} was added to the customer directory.` })
    } catch (reason) {
      setNotice({ kind: 'error', text: `${reason instanceof Error ? reason.message : 'The customer could not be saved.'} Retry without changing the details to avoid a duplicate.` })
    } finally { setCreatingCustomer(false) }
  }

  function beginEdit(customer: HostedCustomer) {
    setEditingCustomer(customer)
    setForm({ name: customer.name, phone: customer.phone ?? '', email: customer.email ?? '', notes: customer.notes ?? '' })
    setNotice(null)
  }

  async function saveEdit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!editingCustomer) return
    setBusyCustomer(editingCustomer.id)
    setNotice(null)
    try {
      const updated = await updateBranchHostedCustomer({
        apiBaseUrl, accessToken, tenantId, branchId, customerId: editingCustomer.id,
        expectedRevision: editingCustomer.revision,
        customer: { name: form.name, phone: form.phone || null, email: form.email || null, notes: form.notes || null },
      })
      setCustomers((current) => current.map((row) => row.id === updated.id ? updated : row))
      setEditingCustomer(null)
      setNotice({ kind: 'success', text: `${updated.name} was updated.` })
    } catch (reason) {
      setNotice({ kind: 'error', text: reason instanceof Error ? reason.message : 'The customer could not be updated. Reload and try again.' })
    } finally { setBusyCustomer(null) }
  }

  async function changeStatus(customer: HostedCustomer) {
    if (customer.status === 'merged') return
    const status = customer.status === 'active' ? 'archived' : 'active'
    setBusyCustomer(customer.id)
    setNotice(null)
    try {
      const updated = await setBranchHostedCustomerStatus({ apiBaseUrl, accessToken, tenantId, branchId, customerId: customer.id, expectedRevision: customer.revision, status })
      if (!includeArchived && updated.status === 'archived') setCustomers((current) => current.filter((row) => row.id !== updated.id))
      else setCustomers((current) => current.map((row) => row.id === updated.id ? updated : row))
      setNotice({ kind: 'success', text: `${updated.name} was ${updated.status === 'archived' ? 'archived' : 'restored'}.` })
    } catch (reason) {
      setNotice({ kind: 'error', text: reason instanceof Error ? reason.message : 'The customer status could not be changed. Reload and try again.' })
    } finally { setBusyCustomer(null) }
  }

  async function mergeCustomer(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!mergeSource || mergeSource.status !== 'active') return
    const target = customers.find((customer) => customer.id === mergeTargetId && customer.status === 'active')
    if (!target) { setNotice({ kind: 'error', text: 'Choose an active target customer from the loaded results.' }); return }
    const attempt = mergeAttempt?.sourceId === mergeSource.id && mergeAttempt.targetId === target.id
      ? mergeAttempt : { sourceId: mergeSource.id, targetId: target.id, key: crypto.randomUUID() }
    setMergeAttempt(attempt)
    setBusyCustomer(mergeSource.id)
    setNotice(null)
    try {
      const result = await mergeBranchHostedCustomer({
        apiBaseUrl, accessToken, tenantId, branchId, sourceCustomerId: mergeSource.id,
        targetCustomerId: target.id, expectedSourceRevision: mergeSource.revision,
        expectedTargetRevision: target.revision, idempotencyKey: attempt.key,
      })
      setCustomers((current) => current.flatMap((customer) => {
        if (customer.id === result.source.id) return includeArchived ? [result.source] : []
        return customer.id === result.target.id ? [result.target] : [customer]
      }))
      setMergeSource(null)
      setMergeTargetId('')
      setConfirmMerge(false)
      setMergeAttempt(null)
      setNotice({ kind: 'success', text: `${result.source.name} was merged into ${result.target.name}. Past sales and receipts keep their original customer snapshots.` })
    } catch (reason) {
      setNotice({ kind: 'error', text: `${reason instanceof Error ? reason.message : 'The customer records could not be merged.'} Retry the same selection to safely reuse this request.` })
    } finally { setBusyCustomer(null) }
  }

  async function openCreditTerms(customer: HostedCustomer) {
    setCreditCustomer(customer)
    setCreditCurrency(currencyCode.toUpperCase())
    setCreditLimit('')
    setCreditDueDays('30')
    setCreditReason('')
    setCreditAttempt(null)
    setCreditTermsLoading(true)
    setNotice(null)
    try {
      const terms = await listTenantHostedCustomerCreditTerms({ apiBaseUrl, accessToken, tenantId, customerId: customer.id })
      setCreditTerms(terms)
      const current = terms.find((term) => term.currencyCode === currencyCode.toUpperCase())
      if (current) {
        setCreditLimit(formatMajorAmount(current.creditLimitMinor, current.currencyCode))
        setCreditDueDays(String(current.dueDays))
      }
    } catch (reason) {
      setCreditCustomer(null)
      setNotice({ kind: 'error', text: reason instanceof Error ? reason.message : 'Credit terms could not be loaded.' })
    } finally { setCreditTermsLoading(false) }
  }

  function selectCreditCurrency(nextCurrency: string) {
    const normalized = nextCurrency.toUpperCase()
    setCreditCurrency(normalized)
    const current = creditTerms.find((term) => term.currencyCode === normalized)
    setCreditLimit(current ? formatMajorAmount(current.creditLimitMinor, normalized) : '')
    setCreditDueDays(current ? String(current.dueDays) : '30')
    setCreditReason('')
    setCreditAttempt(null)
  }

  async function saveCreditTerms(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!creditCustomer) return
    const amountMinor = parseMajorAmount(creditLimit, creditCurrency)
    const dueDays = Number(creditDueDays)
    const existing = creditTerms.find((term) => term.currencyCode === creditCurrency)
    if (amountMinor === null || !Number.isInteger(dueDays) || dueDays < 0 || dueDays > 365 || !creditReason.trim()) {
      setNotice({ kind: 'error', text: 'Enter a valid amount, due period from 0 to 365 days, and reason.' })
      return
    }
    const payload = { creditLimitMinor: amountMinor, dueDays, reason: creditReason.trim(), expectedRevision: existing?.revision ?? 0 }
    const fingerprint = JSON.stringify({ customerId: creditCustomer.id, currencyCode: creditCurrency, ...payload })
    const attempt = creditAttempt?.fingerprint === fingerprint ? creditAttempt : { fingerprint, key: crypto.randomUUID() }
    setCreditAttempt(attempt)
    setSavingCreditTerms(true)
    setNotice(null)
    try {
      const updated = await setTenantHostedCustomerCreditTerm({
        apiBaseUrl, accessToken, tenantId, customerId: creditCustomer.id, currencyCode: creditCurrency,
        idempotencyKey: attempt.key, ...payload,
      })
      setCreditTerms((current) => [updated, ...current.filter((term) => term.currencyCode !== updated.currencyCode)].sort((a, b) => a.currencyCode.localeCompare(b.currencyCode)))
      setCreditDueDays(String(updated.dueDays))
      setCreditReason('')
      setCreditAttempt(null)
      setNotice({ kind: 'success', text: `${updated.currencyCode} credit terms saved at revision ${updated.revision}.` })
      try {
        const history = await listTenantHostedCustomerCreditTermHistory({
          apiBaseUrl, accessToken, tenantId, customerId: creditCustomer.id, currencyCode: updated.currencyCode,
        })
        setCreditHistory(history)
        setCreditHistoryError(null)
      } catch (reason) {
        setCreditHistoryError(reason instanceof Error ? reason.message : 'Credit terms saved, but their history could not be refreshed.')
      }
    } catch (reason) {
      setNotice({ kind: 'error', text: `${reason instanceof Error ? reason.message : 'Credit terms could not be saved.'} Retry without changing the details to safely reuse this request.` })
    } finally { setSavingCreditTerms(false) }
  }

  return <section className="hosted-customers-panel" aria-labelledby="hosted-customers-title">
    <header className="hosted-customers-header">
      <div><p className="eyebrow">CUSTOMER DIRECTORY</p><h2 id="hosted-customers-title">Customers</h2><p>{branchName} · tenant-wide customer records</p></div>
      <span className="customer-directory-icon"><UserRound size={18}/></span>
    </header>
    <div className="hosted-customer-create-toggle"><span>Keep customer contact details ready for future visits and receipts.</span><button className="button button-primary" type="button" onClick={() => setCreateFormOpen((open) => !open)} disabled={creatingCustomer}><UserPlus size={15}/>{createFormOpen ? 'Cancel' : 'Add customer'}</button></div>
    {createFormOpen && <form className="hosted-customer-edit" onSubmit={(event) => void createCustomer(event)} aria-label="Add a customer">
      <div className="hosted-customer-edit-heading"><div><p className="eyebrow">NEW CUSTOMER</p><h3>Add customer</h3></div></div>
      <div className="hosted-customer-fields"><label>Name<input required autoComplete="name" maxLength={160} value={createForm.name} onChange={(event) => setCreateForm((current) => ({ ...current, name: event.currentTarget.value }))} disabled={creatingCustomer}/></label><label>Phone<input autoComplete="tel" maxLength={40} value={createForm.phone} onChange={(event) => setCreateForm((current) => ({ ...current, phone: event.currentTarget.value }))} disabled={creatingCustomer}/></label><label>Email<input autoComplete="email" type="email" maxLength={254} value={createForm.email} onChange={(event) => setCreateForm((current) => ({ ...current, email: event.currentTarget.value }))} disabled={creatingCustomer}/></label><label>Notes<textarea maxLength={1000} value={createForm.notes} onChange={(event) => setCreateForm((current) => ({ ...current, notes: event.currentTarget.value }))} disabled={creatingCustomer}/></label></div>
      <button type="submit" className="button button-primary" disabled={creatingCustomer}>{creatingCustomer ? 'Saving…' : 'Save customer'}</button>
    </form>}
    <form className="hosted-customer-search" onSubmit={search}>
      <label htmlFor="hosted-customer-query">Search customers</label>
      <div><input id="hosted-customer-query" value={query} onChange={(event) => setQuery(event.currentTarget.value)} placeholder="Name, phone or email" maxLength={100}/><button className="button button-primary" type="submit" disabled={loading}><Search size={15}/> Search</button></div>
      <label className="hosted-customer-archived"><input type="checkbox" checked={includeArchived} onChange={(event) => { setOffset(0); setIncludeArchived(event.currentTarget.checked) }}/> Include archived</label>
    </form>
    {notice && <p className={`notice ${notice.kind === 'success' ? 'notice-success' : 'notice-danger'}`} role={notice.kind === 'error' ? 'alert' : 'status'}>{notice.text}</p>}
    {mergeSource && <form className="hosted-customer-edit hosted-customer-merge" onSubmit={(event) => void mergeCustomer(event)} aria-label={`Merge duplicate customer ${mergeSource.name}`}>
      <div className="hosted-customer-edit-heading"><div><p className="eyebrow">MERGE DUPLICATE</p><h3>Merge {mergeSource.name}</h3></div><button type="button" className="button button-quiet button-small" onClick={() => { setMergeSource(null); setMergeTargetId(''); setConfirmMerge(false); setMergeAttempt(null) }} disabled={busyCustomer !== null}>Cancel</button></div>
      <p>The source profile will be marked as merged and linked to the surviving profile. This cannot be undone here. Existing sales and receipt snapshots will not be rewritten.</p>
      <label>Surviving customer<select required value={mergeTargetId} onChange={(event) => setMergeTargetId(event.currentTarget.value)} disabled={busyCustomer !== null}><option value="">Choose an active customer</option>{customers.filter((customer) => customer.status === 'active' && customer.id !== mergeSource.id).map((customer) => <option key={customer.id} value={customer.id}>{customer.name}{customer.phone ? ` · ${customer.phone}` : ''}</option>)}</select></label>
      <label><input type="checkbox" checked={confirmMerge} onChange={(event) => setConfirmMerge(event.currentTarget.checked)} disabled={busyCustomer !== null}/> I have checked the target and want to merge these customer records.</label>
      <button type="submit" className="button button-primary" disabled={busyCustomer !== null || !mergeTargetId || !confirmMerge}>{busyCustomer === mergeSource.id ? 'Merging…' : 'Merge customer records'}</button>
    </form>}
    <div className="hosted-customer-table-wrap"><table className="hosted-customer-table"><thead><tr><th>Customer</th><th>Phone</th><th>Email</th><th>Status</th><th>Actions</th></tr></thead><tbody>
      {customers.map((customer) => <tr key={customer.id}>
        <td><div className="hosted-record-name"><HostedEntityImage apiBaseUrl={apiBaseUrl} accessToken={accessToken} tenantId={tenantId} branchId={branchId} entityType="customer" entityId={customer.id} label={customer.name} canEdit={customer.status !== 'merged'}/><span><strong>{customer.name}</strong>{customer.notes && <small>{customer.notes}</small>}{customer.mergedIntoCustomerId && <small>Merged into customer {customer.mergedIntoCustomerId}</small>}</span></div></td><td>{customer.phone || '—'}</td><td>{customer.email || '—'}</td><td><span className={`customer-status customer-status-${customer.status}`}>{customer.status}</span></td>
        <td><div className="hosted-customer-actions"><button type="button" className="button button-quiet button-small" onClick={() => beginEdit(customer)} disabled={busyCustomer !== null || customer.status !== 'active'}><Edit3 size={13}/> Edit</button>{canManageCreditTerms && customer.status === 'active' && <button type="button" className="button button-quiet button-small" onClick={() => void openCreditTerms(customer)} disabled={busyCustomer !== null || savingCreditTerms}>Credit terms</button>}{customer.status !== 'merged' && <button type="button" className="button button-quiet button-small" onClick={() => void changeStatus(customer)} disabled={busyCustomer !== null}>{customer.status === 'active' ? <><Archive size={13}/> Archive</> : <><RotateCcw size={13}/> Restore</>}</button>}{customer.status === 'active' && <button type="button" className="button button-quiet button-small" onClick={() => { setMergeSource(customer); setMergeTargetId(''); setConfirmMerge(false); setMergeAttempt(null); setNotice(null) }} disabled={busyCustomer !== null}>Merge duplicate</button>}</div></td>
      </tr>)}
    </tbody></table>{!loading && customers.length === 0 && <p className="hosted-customer-empty">No matching {includeArchived ? 'customers' : 'active customers'}.</p>}{loading && <p className="hosted-customer-empty" role="status">Loading customers…</p>}</div>
    <footer className="hosted-customer-pagination"><span>{customers.length === 0 ? 'No records' : `${offset + 1}–${offset + customers.length}`}</span><div><button type="button" className="button button-quiet" onClick={() => setOffset(Math.max(0, offset - PAGE_SIZE))} disabled={offset === 0 || loading}><RefreshCw size={13}/> Previous</button><button type="button" className="button button-quiet" onClick={() => setOffset(offset + PAGE_SIZE)} disabled={customers.length < PAGE_SIZE || loading}>Next</button></div></footer>
    {editingCustomer && <form className="hosted-customer-edit" onSubmit={(event) => void saveEdit(event)} aria-label={`Edit ${editingCustomer.name}`}>
      <div className="hosted-customer-edit-heading"><div><p className="eyebrow">CUSTOMER DETAILS</p><h3>Edit {editingCustomer.name}</h3></div><button type="button" className="button button-quiet button-small" onClick={() => setEditingCustomer(null)} disabled={busyCustomer !== null}>Cancel</button></div>
      <div className="hosted-customer-fields"><label>Name<input required maxLength={160} value={form.name} onChange={(event) => setForm((current) => ({ ...current, name: event.currentTarget.value }))}/></label><label>Phone<input maxLength={40} value={form.phone} onChange={(event) => setForm((current) => ({ ...current, phone: event.currentTarget.value }))}/></label><label>Email<input type="email" maxLength={254} value={form.email} onChange={(event) => setForm((current) => ({ ...current, email: event.currentTarget.value }))}/></label><label>Notes<textarea maxLength={1000} value={form.notes} onChange={(event) => setForm((current) => ({ ...current, notes: event.currentTarget.value }))}/></label></div>
      <button type="submit" className="button button-primary" disabled={busyCustomer !== null}>{busyCustomer === editingCustomer.id ? 'Saving…' : 'Save changes'}</button>
    </form>}
    {creditCustomer && <form className="hosted-customer-edit hosted-credit-terms" onSubmit={(event) => void saveCreditTerms(event)} aria-label={`Credit terms for ${creditCustomer.name}`}>
      <div className="hosted-customer-edit-heading"><div><p className="eyebrow">CUSTOMER CREDIT</p><h3>Credit terms · {creditCustomer.name}</h3><small>Terms apply across this business for the selected currency.</small></div><button type="button" className="button button-quiet button-small" onClick={() => setCreditCustomer(null)} disabled={savingCreditTerms}>Close</button></div>
      {creditTermsLoading ? <p role="status">Loading credit terms…</p> : <>
        {creditTerms.length > 0 && <div className="hosted-credit-term-list" aria-label="Configured currency terms">{creditTerms.map((term) => <button type="button" className={`button button-quiet button-small${term.currencyCode === creditCurrency ? ' is-active' : ''}`} key={term.currencyCode} onClick={() => selectCreditCurrency(term.currencyCode)} disabled={savingCreditTerms}>{term.currencyCode} · {formatMajorAmount(term.creditLimitMinor, term.currencyCode)} · {term.dueDays} days</button>)}</div>}
        <div className="hosted-customer-fields">
          <label>Currency code<input required minLength={3} maxLength={3} pattern="[A-Za-z]{3}" value={creditCurrency} onChange={(event) => selectCreditCurrency(event.currentTarget.value)} disabled={savingCreditTerms}/></label>
          <label>Credit limit<input required inputMode="decimal" value={creditLimit} onChange={(event) => setCreditLimit(event.currentTarget.value)} placeholder={formatMajorAmount('0', creditCurrency)} disabled={savingCreditTerms}/></label>
          <label>Due in days<input required type="number" min={0} max={365} step={1} value={creditDueDays} onChange={(event) => setCreditDueDays(event.currentTarget.value)} disabled={savingCreditTerms}/></label>
          <label>Reason for this limit<input required maxLength={500} value={creditReason} onChange={(event) => setCreditReason(event.currentTarget.value)} placeholder="For example, approved monthly account terms" disabled={savingCreditTerms}/></label>
        </div>
        <p className="hosted-credit-terms-current">{creditTerms.find((term) => term.currencyCode === creditCurrency) ? `Current revision ${creditTerms.find((term) => term.currencyCode === creditCurrency)?.revision}.` : `No ${creditCurrency} terms yet. Saving creates revision 1.`} A limit of zero disables credit sales.</p>
        <button type="submit" className="button button-primary" disabled={savingCreditTerms}>{savingCreditTerms ? 'Saving…' : 'Save credit terms'}</button>
        <section className="hosted-credit-history" aria-labelledby="hosted-credit-history-title">
          <h4 id="hosted-credit-history-title">Change history · {creditCurrency}</h4>
          {creditHistoryLoading ? <p role="status">Loading credit term history…</p>
            : creditHistoryError ? <p className="notice notice-danger" role="alert">{creditHistoryError}</p>
              : creditHistory.length === 0 ? <p>No changes recorded for this currency.</p>
                : <ol>{creditHistory.map((event) => <li key={`${event.currencyCode}-${event.revision}`}>
                  <div><strong>Revision {event.revision}</strong><time dateTime={event.createdAt}>{formatHistoryDate(event.createdAt)}</time></div>
                  <p>{event.previousCreditLimitMinor === null
                    ? `Initial limit ${formatMajorAmount(event.creditLimitMinor, event.currencyCode)} · ${event.dueDays} days`
                    : `${formatMajorAmount(event.previousCreditLimitMinor, event.currencyCode)} → ${formatMajorAmount(event.creditLimitMinor, event.currencyCode)} · due ${event.previousDueDays} → ${event.dueDays} days`}</p>
                  <p>{event.reason}</p><small>Changed by {event.actorName}</small>
                </li>)}</ol>}
        </section>
      </>}
    </form>}
  </section>
}

function currencyDigits(currencyCode: string): number {
  try {
    return new Intl.NumberFormat(undefined, { style: 'currency', currency: currencyCode }).resolvedOptions().maximumFractionDigits ?? 2
  } catch {
    return 2
  }
}

function parseMajorAmount(value: string, currencyCode: string): number | null {
  const normalized = value.trim()
  const digits = currencyDigits(currencyCode)
  if (!/^\d+(?:\.\d+)?$/.test(normalized) || (normalized.split('.')[1]?.length ?? 0) > digits) return null
  const amount = Number(normalized) * 10 ** digits
  return Number.isSafeInteger(amount) && amount <= 9_007_199_254_740_991 ? amount : null
}

function formatMajorAmount(minorText: string, currencyCode: string): string {
  const minor = Number(minorText)
  if (!Number.isSafeInteger(minor)) return ''
  return (minor / 10 ** currencyDigits(currencyCode)).toFixed(currencyDigits(currencyCode))
}

function formatHistoryDate(value: string): string {
  const date = new Date(value)
  return Number.isNaN(date.getTime()) ? value : new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' }).format(date)
}
