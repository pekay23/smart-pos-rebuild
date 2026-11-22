// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useCallback, useEffect, useRef, useState } from 'react'
import { ArrowDownToLine, ArrowRightLeft, Check, CircleAlert, RefreshCw, Send, X } from 'lucide-react'
import { listHostedStockTransfers, transitionHostedStockTransfer } from './api'
import type { HostedStockTransfer } from './api'

type Notice = { kind: 'error' | 'success'; message: string } | null
const stateLabels: Record<string, string> = { requested: 'Requested', accepted: 'Accepted', rejected: 'Rejected', in_transit: 'In transit', partially_received: 'Partially received', received: 'Received', discrepancy: 'Short shipment', discrepancy_resolved: 'Discrepancy resolved', cancelled: 'Cancelled' }

export function BranchStockTransfersPanel({ apiBaseUrl, accessToken, tenantId, branchId, branchName, canManage }: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; branchName: string; canManage: boolean
}) {
  const [transfers, setTransfers] = useState<HostedStockTransfer[]>([])
  const [loading, setLoading] = useState(true)
  const [busyId, setBusyId] = useState<string | null>(null)
  const [note, setNote] = useState('')
  const [quantityEdits, setQuantityEdits] = useState<Record<string, Record<string, string>>>({})
  const [notice, setNotice] = useState<Notice>(null)
  const transitionAttempts = useRef(new Map<string, { fingerprint: string; key: string }>())

  const refresh = useCallback(async () => {
    setLoading(true)
    try { setTransfers(await listHostedStockTransfers({ apiBaseUrl, accessToken, tenantId, branchId })); setNotice(null) }
    catch (reason) { setNotice({ kind: 'error', message: reason instanceof Error ? reason.message : 'Transfers could not be loaded.' }) }
    finally { setLoading(false) }
  }, [accessToken, apiBaseUrl, branchId, tenantId])
  useEffect(() => { void refresh() }, [refresh])

  async function transition(transfer: HostedStockTransfer, action: 'accept' | 'reject' | 'dispatch' | 'receive' | 'resolve_discrepancy') {
    const quantities = transfer.items.map((item) => {
      const maximum = action === 'accept' ? item.requestedQuantity
        : action === 'dispatch' ? (item.acceptedQuantity ?? item.requestedQuantity)
          : action === 'receive' ? Math.max(0, (item.dispatchedQuantity ?? 0) - (item.receivedQuantity ?? 0))
            : 0
      const edited = quantityEdits[transfer.transferId]?.[item.productId]
      return { productId: item.productId, quantity: edited === undefined ? maximum : Number(edited), maximum }
    })
    if (!['reject', 'resolve_discrepancy'].includes(action) && quantities.some(({ quantity, maximum }) => !Number.isSafeInteger(quantity) || quantity < (action === 'receive' ? 0 : 1) || quantity > maximum)) {
      setNotice({ kind: 'error', message: 'Enter a whole number within the allowed quantity for every item.' }); return
    }
    if (action === 'reject' && !note.trim()) {
      setNotice({ kind: 'error', message: 'Add a reason before rejecting a request.' }); return
    }
    if (action === 'dispatch' && quantities.some(({ quantity, maximum }) => quantity < maximum) && !note.trim()) {
      setNotice({ kind: 'error', message: 'Add a reason before releasing any accepted quantity that will not be dispatched.' }); return
    }
    if (action === 'receive' && !quantities.some(({ quantity }) => quantity > 0)) {
      setNotice({ kind: 'error', message: 'Enter at least one unit received in this delivery.' }); return
    }
    if (action === 'resolve_discrepancy' && !note.trim()) {
      setNotice({ kind: 'error', message: 'Record how the short shipment was resolved before closing it.' }); return
    }
    setBusyId(transfer.transferId); setNotice(null)
    const items = action === 'reject' || action === 'resolve_discrepancy' ? [] : quantities.map(({ productId, quantity }) => ({ productId, quantity }))
    const fingerprint = JSON.stringify({ action, revision: transfer.revision, note: note.trim(), items })
    const attemptKey = `${transfer.transferId}:${action}`
    let attempt = transitionAttempts.current.get(attemptKey)
    if (!attempt || attempt.fingerprint !== fingerprint) {
      attempt = { fingerprint, key: crypto.randomUUID() }
      transitionAttempts.current.set(attemptKey, attempt)
    }
    try {
      await transitionHostedStockTransfer({ apiBaseUrl, accessToken, tenantId, branchId, transferId: transfer.transferId, action, revision: transfer.revision, note: note.trim(), items, idempotencyKey: attempt.key })
      transitionAttempts.current.delete(attemptKey)
      const pastTense = { accept: 'accepted', reject: 'rejected', dispatch: 'dispatched', receive: 'receipt recorded', resolve_discrepancy: 'discrepancy resolved' }[action]
      setNote('')
      setQuantityEdits((current) => { const next = { ...current }; delete next[transfer.transferId]; return next })
      await refresh(); setNotice({ kind: 'success', message: `Transfer ${pastTense}.` })
    } catch (reason) { setNotice({ kind: 'error', message: reason instanceof Error ? reason.message : 'Transfer update failed. Refresh and try again.' }) }
    finally { setBusyId(null) }
  }

  return <section className="branch-transfer-panel" aria-labelledby="branch-transfer-title">
    <header className="branch-transfer-header"><div><p className="eyebrow">INVENTORY MOVEMENT</p><h2 id="branch-transfer-title">Branch stock transfers</h2><p>Review requests involving {branchName}. Source acceptance reserves stock; dispatch moves it, and receipt adds only goods actually received.</p></div><button type="button" className="button button-quiet" onClick={() => void refresh()} disabled={loading}><RefreshCw size={15}/> Refresh</button></header>
    {notice && <div className={`notice ${notice.kind === 'error' ? 'notice-danger' : 'notice-success'}`} role={notice.kind === 'error' ? 'alert' : 'status'}>{notice.kind === 'error' ? <CircleAlert size={16}/> : <Check size={16}/>}<span>{notice.message}</span></div>}
    {canManage && <label className="transfer-note-field">Action note <input value={note} onChange={(event) => setNote(event.target.value)} maxLength={500} placeholder="Required to reject, reduce dispatch, or close/resolve a shortage" /></label>}
    {loading ? <div className="catalog-state"><span className="mini-spinner"/>Loading transfers…</div> : transfers.length === 0 ? <div className="empty-inline">No transfer requests yet.</div> : <div className="transfer-list">{transfers.map((transfer) => {
      const destination = transfer.destinationBranchId === branchId
      const active = busyId === transfer.transferId
      return <article className="transfer-card" key={transfer.transferId}>
        <div className="transfer-card-heading"><span className="transfer-direction"><ArrowRightLeft size={15}/>{destination ? 'Incoming request' : 'Outgoing request'}</span><span className={`transfer-status transfer-status-${transfer.status}`}>{stateLabels[transfer.status] ?? transfer.status}</span></div>
        <small>{new Date(transfer.requestedAt).toLocaleString()} · {destination ? 'From another branch' : 'To another branch'} · Revision {transfer.revision}</small>
        {transfer.requestNote && <p className="transfer-request-note">{transfer.requestNote}</p>}
        <div className="transfer-lines">{transfer.items.map((item) => {
          const receiptOpen = destination && ['in_transit', 'partially_received', 'discrepancy'].includes(transfer.status)
          const quantityLabel = !destination && transfer.status === 'requested' ? 'Accept' : !destination && transfer.status === 'accepted' ? 'Dispatch' : receiptOpen ? 'Receive now' : null
          const maxQuantity = transfer.status === 'requested' ? item.requestedQuantity : transfer.status === 'accepted' ? (item.acceptedQuantity ?? item.requestedQuantity) : Math.max(0, (item.dispatchedQuantity ?? 0) - (item.receivedQuantity ?? 0))
          const defaultQuantity = maxQuantity
          return <div key={item.productId}><span><strong>{item.name}</strong><small>{item.sku}</small></span><span className="transfer-line-quantities">Requested {item.requestedQuantity}{item.acceptedQuantity !== null ? ` · accepted ${item.acceptedQuantity}` : ''}{item.dispatchedQuantity !== null ? ` · sent ${item.dispatchedQuantity}` : ''}{item.receivedQuantity !== null ? ` · received ${item.receivedQuantity}/${item.dispatchedQuantity ?? 0}` : ''}{quantityLabel && maxQuantity > 0 && <label className="transfer-quantity-edit">{quantityLabel} <input type="number" min={quantityLabel === 'Receive now' ? 0 : 1} max={maxQuantity} value={quantityEdits[transfer.transferId]?.[item.productId] ?? String(defaultQuantity)} onChange={(event) => setQuantityEdits((current) => ({ ...current, [transfer.transferId]: { ...current[transfer.transferId], [item.productId]: event.target.value } }))} aria-label={`${quantityLabel} quantity for ${item.name}`} /></label>}</span></div>
        } )}</div>
        {['discrepancy', 'discrepancy_resolved'].includes(transfer.status) && transfer.items.some((item) => item.discrepancyNote) && <p className="transfer-discrepancy">{transfer.status === 'discrepancy_resolved' ? 'Resolution recorded: ' : 'Short shipment: '}{transfer.items.find((item) => item.discrepancyNote)?.discrepancyNote}</p>}
        {canManage && <div className="transfer-actions">
          {!destination && transfer.status === 'requested' && <><button className="button button-primary" disabled={active} onClick={() => void transition(transfer, 'accept')}><Check size={14}/> Accept quantities</button><button className="button button-quiet" disabled={active} onClick={() => void transition(transfer, 'reject')}><X size={14}/> Reject</button></>}
          {!destination && transfer.status === 'accepted' && <button className="button button-primary" disabled={active} onClick={() => void transition(transfer, 'dispatch')}><Send size={14}/> Dispatch stock</button>}
          {destination && ['in_transit', 'partially_received', 'discrepancy'].includes(transfer.status) && <button className="button button-primary" disabled={active || !transfer.items.some((item) => (item.dispatchedQuantity ?? 0) > (item.receivedQuantity ?? 0))} onClick={() => void transition(transfer, 'receive')}><ArrowDownToLine size={14}/> Record this delivery</button>}
          {destination && ['partially_received', 'discrepancy'].includes(transfer.status) && <button className="button button-quiet" disabled={active} onClick={() => void transition(transfer, 'resolve_discrepancy')}><Check size={14}/> Resolve and close shortage</button>}
          {active && <span className="transfer-processing">Saving…</span>}
        </div>}
      </article>
    })}</div>}
    <p className="transfer-disclosure">Receipt quantities are added per delivery and shown cumulatively. Close a remaining shortage only after investigating it; this records the resolution and does not add stock.</p>
  </section>
}
