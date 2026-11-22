// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useEffect, useState } from 'react'
import type { FormEvent } from 'react'
import { Archive, Edit3, RefreshCw, RotateCcw, Search, Truck } from 'lucide-react'
import { createBranchHostedSupplier, listBranchHostedSuppliers, setBranchHostedSupplierStatus, updateBranchHostedSupplier } from './api'
import type { HostedSupplier, HostedSupplierInput } from './api'
import { HostedEntityImage } from './HostedEntityImage'

const PAGE_SIZE = 50
const emptyForm = (): HostedSupplierInput => ({ name: '', contactPerson: null, email: null, phone: null, address: null, notes: null })

export function HostedSuppliersPanel({ apiBaseUrl, accessToken, tenantId, branchId, branchName }: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; branchName: string
}) {
  const [rows, setRows] = useState<HostedSupplier[]>([])
  const [query, setQuery] = useState('')
  const [appliedQuery, setAppliedQuery] = useState('')
  const [includeArchived, setIncludeArchived] = useState(false)
  const [offset, setOffset] = useState(0)
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState<string | null>(null)
  const [editing, setEditing] = useState<HostedSupplier | null>(null)
  const [formOpen, setFormOpen] = useState(false)
  const [form, setForm] = useState<HostedSupplierInput>(emptyForm())
  const [editForm, setEditForm] = useState<HostedSupplierInput>(emptyForm())
  const [createAttempt, setCreateAttempt] = useState<{ fingerprint: string; key: string } | null>(null)
  const [notice, setNotice] = useState<{ kind: 'success' | 'error'; text: string } | null>(null)

  useEffect(() => {
    let current = true
    setLoading(true)
    void listBranchHostedSuppliers({ apiBaseUrl, accessToken, tenantId, branchId, query: appliedQuery, offset, pageSize: PAGE_SIZE, includeArchived })
      .then((suppliers) => { if (current) setRows(suppliers) })
      .catch((reason: unknown) => { if (current) setNotice({ kind: 'error', text: reason instanceof Error ? reason.message : 'Supplier records could not be loaded.' }) })
      .finally(() => { if (current) setLoading(false) })
    return () => { current = false }
  }, [accessToken, apiBaseUrl, appliedQuery, branchId, includeArchived, offset, tenantId])

  function search(event: FormEvent<HTMLFormElement>) { event.preventDefault(); setOffset(0); setAppliedQuery(query.trim()) }
  function normalized(source: HostedSupplierInput): HostedSupplierInput {
    return { name: source.name.trim(), contactPerson: source.contactPerson?.trim() || null, email: source.email?.trim() || null, phone: source.phone?.trim() || null, address: source.address?.trim() || null, notes: source.notes?.trim() || null }
  }
  async function createSupplier(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const supplier = normalized(form)
    if (!supplier.name) { setNotice({ kind: 'error', text: 'Enter the supplier name.' }); return }
    const fingerprint = JSON.stringify(supplier)
    const attempt = createAttempt?.fingerprint === fingerprint ? createAttempt : { fingerprint, key: crypto.randomUUID() }
    setCreateAttempt(attempt); setBusy('create'); setNotice(null)
    try {
      const created = await createBranchHostedSupplier({ apiBaseUrl, accessToken, tenantId, branchId, idempotencyKey: attempt.key, supplier })
      setCreateAttempt(null); setForm(emptyForm()); setFormOpen(false); setQuery(''); setAppliedQuery(''); setIncludeArchived(false); setOffset(0)
      setRows((current) => [created, ...current.filter((row) => row.id !== created.id)].slice(0, PAGE_SIZE))
      setNotice({ kind: 'success', text: `${created.name} was added to the supplier directory.` })
    } catch (reason) { setNotice({ kind: 'error', text: `${reason instanceof Error ? reason.message : 'The supplier could not be saved.'} Retry without changing the details to avoid a duplicate.` }) }
    finally { setBusy(null) }
  }
  function beginEdit(row: HostedSupplier) {
    setEditing(row)
    setEditForm({ name: row.name, contactPerson: row.contactPerson, email: row.email, phone: row.phone, address: row.address, notes: row.notes })
    setNotice(null)
  }
  async function saveEdit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); if (!editing) return
    setBusy(editing.id); setNotice(null)
    try {
      const updated = await updateBranchHostedSupplier({ apiBaseUrl, accessToken, tenantId, branchId, supplierId: editing.id, expectedRevision: editing.revision, supplier: normalized(editForm) })
      setRows((current) => current.map((row) => row.id === updated.id ? updated : row)); setEditing(null)
      setNotice({ kind: 'success', text: `${updated.name} was updated.` })
    } catch (reason) { setNotice({ kind: 'error', text: reason instanceof Error ? reason.message : 'The supplier could not be updated. Reload and try again.' }) }
    finally { setBusy(null) }
  }
  async function changeStatus(row: HostedSupplier) {
    const status = row.status === 'active' ? 'archived' : 'active'
    setBusy(row.id); setNotice(null)
    try {
      const updated = await setBranchHostedSupplierStatus({ apiBaseUrl, accessToken, tenantId, branchId, supplierId: row.id, expectedRevision: row.revision, status })
      if (!includeArchived && updated.status === 'archived') setRows((current) => current.filter((item) => item.id !== row.id))
      else setRows((current) => current.map((item) => item.id === row.id ? updated : item))
      setNotice({ kind: 'success', text: `${updated.name} was ${status}.` })
    } catch (reason) { setNotice({ kind: 'error', text: reason instanceof Error ? reason.message : 'The supplier status could not be changed. Reload and try again.' }) }
    finally { setBusy(null) }
  }
  function fields(value: HostedSupplierInput, setValue: (next: HostedSupplierInput) => void, disabled: boolean) {
    const set = (key: keyof HostedSupplierInput, next: string) => setValue({ ...value, [key]: next })
    return <div className="hosted-customer-fields">
      <label>Supplier name<input required maxLength={160} value={value.name} onChange={(event) => set('name', event.currentTarget.value)} disabled={disabled}/></label>
      <label>Contact person<input autoComplete="name" maxLength={160} value={value.contactPerson ?? ''} onChange={(event) => set('contactPerson', event.currentTarget.value)} disabled={disabled}/></label>
      <label>Phone<input autoComplete="tel" maxLength={40} value={value.phone ?? ''} onChange={(event) => set('phone', event.currentTarget.value)} disabled={disabled}/></label>
      <label>Email<input autoComplete="email" type="email" maxLength={254} value={value.email ?? ''} onChange={(event) => set('email', event.currentTarget.value)} disabled={disabled}/></label>
      <label>Address<input autoComplete="street-address" maxLength={500} value={value.address ?? ''} onChange={(event) => set('address', event.currentTarget.value)} disabled={disabled}/></label>
      <label>Notes<textarea maxLength={1000} value={value.notes ?? ''} onChange={(event) => set('notes', event.currentTarget.value)} disabled={disabled}/></label>
    </div>
  }

  return <section className="hosted-customers-panel" aria-labelledby="hosted-suppliers-title">
    <header className="hosted-customers-header"><div><p className="eyebrow">SUPPLIER DIRECTORY</p><h2 id="hosted-suppliers-title">Suppliers</h2><p>{branchName} · tenant-wide supplier records</p></div><span className="customer-directory-icon"><Truck size={18}/></span></header>
    <div className="hosted-customer-create-toggle"><span>Keep supplier contacts organized for purchasing and receiving workflows.</span><button className="button button-primary" type="button" onClick={() => setFormOpen((open) => !open)} disabled={busy !== null}>{formOpen ? 'Cancel' : 'Add supplier'}</button></div>
    {formOpen && <form className="hosted-customer-edit" onSubmit={(event) => void createSupplier(event)} aria-label="Add a supplier"><div className="hosted-customer-edit-heading"><div><p className="eyebrow">NEW SUPPLIER</p><h3>Add supplier</h3></div></div>{fields(form, setForm, busy !== null)}<button type="submit" className="button button-primary" disabled={busy !== null}>{busy === 'create' ? 'Saving…' : 'Save supplier'}</button></form>}
    <form className="hosted-customer-search" onSubmit={search}><label htmlFor="hosted-supplier-query">Search suppliers</label><div><input id="hosted-supplier-query" value={query} onChange={(event) => setQuery(event.currentTarget.value)} placeholder="Name, contact, phone, email or address" maxLength={100}/><button className="button button-primary" type="submit" disabled={loading}><Search size={15}/> Search</button></div><label className="hosted-customer-archived"><input type="checkbox" checked={includeArchived} onChange={(event) => { setOffset(0); setIncludeArchived(event.currentTarget.checked) }}/> Include archived</label></form>
    {notice && <p className={`notice ${notice.kind === 'success' ? 'notice-success' : 'notice-danger'}`} role={notice.kind === 'error' ? 'alert' : 'status'}>{notice.text}</p>}
    <div className="hosted-customer-table-wrap"><table className="hosted-customer-table"><thead><tr><th>Supplier</th><th>Contact</th><th>Phone</th><th>Email</th><th>Status</th><th>Actions</th></tr></thead><tbody>
      {rows.map((row) => <tr key={row.id}><td><div className="hosted-record-name"><HostedEntityImage apiBaseUrl={apiBaseUrl} accessToken={accessToken} tenantId={tenantId} branchId={branchId} entityType="supplier" entityId={row.id} label={row.name} canEdit/><span><strong>{row.name}</strong>{row.address && <small>{row.address}</small>}{row.notes && <small>{row.notes}</small>}</span></div></td><td>{row.contactPerson || '—'}</td><td>{row.phone || '—'}</td><td>{row.email || '—'}</td><td><span className={`customer-status customer-status-${row.status}`}>{row.status}</span></td><td><div className="hosted-customer-actions"><button type="button" className="button button-quiet button-small" onClick={() => beginEdit(row)} disabled={busy !== null || row.status !== 'active'}><Edit3 size={13}/> Edit</button><button type="button" className="button button-quiet button-small" onClick={() => void changeStatus(row)} disabled={busy !== null}>{row.status === 'active' ? <><Archive size={13}/> Archive</> : <><RotateCcw size={13}/> Restore</>}</button></div></td></tr>)}
    </tbody></table>{!loading && rows.length === 0 && <p className="hosted-customer-empty">No matching {includeArchived ? 'suppliers' : 'active suppliers'}.</p>}{loading && <p className="hosted-customer-empty" role="status">Loading suppliers…</p>}</div>
    <footer className="hosted-customer-pagination"><span>{rows.length === 0 ? 'No records' : `${offset + 1}–${offset + rows.length}`}</span><div><button type="button" className="button button-quiet" onClick={() => setOffset(Math.max(0, offset - PAGE_SIZE))} disabled={offset === 0 || loading}><RefreshCw size={13}/> Previous</button><button type="button" className="button button-quiet" onClick={() => setOffset(offset + PAGE_SIZE)} disabled={rows.length < PAGE_SIZE || loading}>Next</button></div></footer>
    {editing && <form className="hosted-customer-edit" onSubmit={(event) => void saveEdit(event)} aria-label={`Edit ${editing.name}`}><div className="hosted-customer-edit-heading"><div><p className="eyebrow">SUPPLIER DETAILS</p><h3>Edit {editing.name}</h3></div><button type="button" className="button button-quiet" onClick={() => setEditing(null)} disabled={busy !== null}>Cancel</button></div>{fields(editForm, setEditForm, busy !== null)}<button type="submit" className="button button-primary" disabled={busy !== null}>{busy === editing.id ? 'Saving…' : 'Save changes'}</button></form>}
  </section>
}
