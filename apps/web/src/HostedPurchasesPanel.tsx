// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useCallback, useEffect, useState } from 'react'
import type { FormEvent } from 'react'
import { Archive, Check, ClipboardList, FileCheck2, PackageCheck, Pencil, Plus, Truck } from 'lucide-react'
import { getHostedCashLocations, listBranchHostedSuppliers, listBranchProducts } from './api'
import { createBranchPurchaseOrder, createBranchSupplierCreditNote, getBranchSupplierBilling, listBranchPurchaseOrders, matchBranchSupplierBill, receiveBranchPurchaseOrder, recordBranchSupplierPayment, transitionBranchPurchaseOrder, updateBranchPurchaseOrderDraft } from './api'
import type { BranchProduct, HostedCashLocation, HostedPurchaseOrder, HostedPurchaseOrderItemInput, HostedPurchaseReceiptItemInput, HostedSupplier, HostedSupplierBillingWorkspace, HostedSupplierCreditLineInput, HostedSupplierPayment } from './api'

type DraftLine = { key: number; productId: string; quantity: string; expectedCost: string }
type ReceiptLine = HostedPurchaseReceiptItemInput & { name: string }
const newDraftLine = (key: number): DraftLine => ({ key, productId: '', quantity: '1', expectedCost: '' })
const moneyInput = (minor: number) => (minor / 100).toFixed(2)
function parseMoneyInput(value: string): number | null {
  const normalized = value.trim().replace(/,/g, '')
  if (!/^\d+(\.\d{1,2})?$/.test(normalized)) return null
  const minor = Math.round(Number(normalized) * 100)
  return Number.isSafeInteger(minor) ? minor : null
}
function formatMinor(minor: number, currency: string): string {
  return new Intl.NumberFormat(undefined, { style: 'currency', currency }).format(minor / 100)
}
function isSupplierPaymentMethod(value: string): value is HostedSupplierPayment['paymentMethod'] {
  return value === 'cash' || value === 'card' || value === 'mobile_money' || value === 'bank_transfer' || value === 'other'
}

export function HostedPurchasesPanel({ apiBaseUrl, accessToken, tenantId, branchId, branchName, currencyCode, canWrite, canInvoice, canSettle }: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; branchName: string; currencyCode: string; canWrite: boolean; canInvoice: boolean; canSettle: boolean
}) {
  const [orders, setOrders] = useState<HostedPurchaseOrder[]>([])
  const [products, setProducts] = useState<BranchProduct[]>([])
  const [suppliers, setSuppliers] = useState<HostedSupplier[]>([])
  const [billing, setBilling] = useState<HostedSupplierBillingWorkspace | null>(null)
  const [cashLocations, setCashLocations] = useState<HostedCashLocation[]>([])
  const [loading, setLoading] = useState(true)
  const [formOpen, setFormOpen] = useState(false)
  const [editing, setEditing] = useState<HostedPurchaseOrder | null>(null)
  const [supplierId, setSupplierId] = useState('')
  const [reference, setReference] = useState('')
  const [lines, setLines] = useState<DraftLine[]>([newDraftLine(1)])
  const [nextLineKey, setNextLineKey] = useState(2)
  const [busy, setBusy] = useState<string | null>(null)
  const [notice, setNotice] = useState<{ kind: 'success' | 'error'; text: string } | null>(null)
  const [createAttempt, setCreateAttempt] = useState<{ fingerprint: string; key: string } | null>(null)
  const [receiving, setReceiving] = useState<HostedPurchaseOrder | null>(null)
  const [receiptReference, setReceiptReference] = useState('')
  const [receiptLines, setReceiptLines] = useState<ReceiptLine[]>([])
  const [receiptAttempt, setReceiptAttempt] = useState<{ fingerprint: string; key: string } | null>(null)
  const [billSupplierId, setBillSupplierId] = useState('')
  const [billReference, setBillReference] = useState('')
  const [invoiceDate, setInvoiceDate] = useState(() => new Date().toISOString().slice(0, 10))
  const [dueDate, setDueDate] = useState('')
  const [invoiceTotal, setInvoiceTotal] = useState('')
  const [receiptAllocations, setReceiptAllocations] = useState<Record<string, string>>({})
  const [billAttempt, setBillAttempt] = useState<{ fingerprint: string; key: string } | null>(null)
  const [paymentSupplierId, setPaymentSupplierId] = useState('')
  const [paymentMethod, setPaymentMethod] = useState<HostedSupplierPayment['paymentMethod']>('bank_transfer')
  const [paymentCashLocationId, setPaymentCashLocationId] = useState('')
  const [paymentReference, setPaymentReference] = useState('')
  const [paymentAllocations, setPaymentAllocations] = useState<Record<string, string>>({})
  const [paymentAttempt, setPaymentAttempt] = useState<{ fingerprint: string; key: string } | null>(null)
  const [creditBillId, setCreditBillId] = useState('')
  const [creditReference, setCreditReference] = useState('')
  const [creditDate, setCreditDate] = useState(() => new Date().toISOString().slice(0, 10))
  const [creditReason, setCreditReason] = useState('Goods returned to supplier')
  const [returnQuantities, setReturnQuantities] = useState<Record<string, string>>({})
  const [creditAttempt, setCreditAttempt] = useState<{ fingerprint: string; key: string } | null>(null)

  const reload = useCallback(async () => {
    setLoading(true)
    try {
      const [nextOrders, nextProducts, nextSuppliers, nextBilling] = await Promise.all([
        listBranchPurchaseOrders({ apiBaseUrl, accessToken, tenantId, branchId }),
        listBranchProducts({ apiBaseUrl, accessToken, tenantId, branchId }),
        listBranchHostedSuppliers({ apiBaseUrl, accessToken, tenantId, branchId, pageSize: 100 }),
        getBranchSupplierBilling({ apiBaseUrl, accessToken, tenantId, branchId }),
      ])
      setOrders(nextOrders); setProducts(nextProducts); setSuppliers(nextSuppliers.filter((item) => item.status === 'active')); setBilling(nextBilling)
    } catch (reason) { setNotice({ kind: 'error', text: reason instanceof Error ? reason.message : 'Purchasing data could not be loaded.' }) }
    finally { setLoading(false) }
  }, [accessToken, apiBaseUrl, branchId, tenantId])
  useEffect(() => { void reload() }, [reload])

  useEffect(() => {
    let current = true
    void getHostedCashLocations({ apiBaseUrl, accessToken, tenantId, branchId })
      .then((result) => { if (current) setCashLocations(result.locations.filter((location) => location.status === 'active' && location.initialized && location.locationType !== 'register')) })
      .catch(() => { if (current) setCashLocations([]) })
    return () => { current = false }
  }, [accessToken, apiBaseUrl, branchId, tenantId])

  function clearDraft() {
    setEditing(null); setSupplierId(''); setReference(''); setLines([newDraftLine(nextLineKey)]); setNextLineKey((value) => value + 1); setFormOpen(false)
  }
  function startEdit(order: HostedPurchaseOrder) {
    setEditing(order); setSupplierId(order.supplierId ?? ''); setReference(order.reference ?? '')
    setLines(order.items.map((line, index) => ({ key: index + 1, productId: line.productId, quantity: String(line.orderedQuantity), expectedCost: moneyInput(line.expectedUnitCostMinor) })))
    setNextLineKey(order.items.length + 1); setFormOpen(true); setNotice(null)
  }
  function parseOrderLines(): HostedPurchaseOrderItemInput[] | null {
    const parsed = lines.map((line) => ({ productId: line.productId, quantity: Number(line.quantity), expectedUnitCostMinor: parseMoneyInput(line.expectedCost) }))
    if (parsed.some((line) => !line.productId || !Number.isSafeInteger(line.quantity) || line.quantity < 1 || line.quantity > 1_000_000 || line.expectedUnitCostMinor === null)) return null
    const productIds = parsed.map((line) => line.productId)
    if (new Set(productIds).size !== productIds.length) return null
    return parsed.map((line) => ({ productId: line.productId, quantity: line.quantity, expectedUnitCostMinor: line.expectedUnitCostMinor! }))
  }
  async function saveDraft(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const items = parseOrderLines()
    if (!items) { setNotice({ kind: 'error', text: 'Choose unique products and enter valid quantities and estimated unit costs.' }); return }
    const payload = { supplierId: supplierId || null, reference: reference.trim() || null, items }
    setBusy(editing?.id ?? 'create'); setNotice(null)
    try {
      if (editing) {
        await updateBranchPurchaseOrderDraft({ apiBaseUrl, accessToken, tenantId, branchId, orderId: editing.id, expectedRevision: editing.revision, ...payload })
      } else {
        const fingerprint = JSON.stringify(payload)
        const attempt = createAttempt?.fingerprint === fingerprint ? createAttempt : { fingerprint, key: crypto.randomUUID() }
        setCreateAttempt(attempt)
        await createBranchPurchaseOrder({ apiBaseUrl, accessToken, tenantId, branchId, idempotencyKey: attempt.key, ...payload })
        setCreateAttempt(null)
      }
      clearDraft(); await reload(); setNotice({ kind: 'success', text: editing ? 'Purchase order draft updated.' : 'Purchase order draft saved.' })
    } catch (reason) { setNotice({ kind: 'error', text: `${reason instanceof Error ? reason.message : 'The draft could not be saved.'} Retry without changing the details to avoid a duplicate.` }) }
    finally { setBusy(null) }
  }
  async function transition(order: HostedPurchaseOrder, action: 'place' | 'cancel') {
    setBusy(order.id); setNotice(null)
    try { await transitionBranchPurchaseOrder({ apiBaseUrl, accessToken, tenantId, branchId, orderId: order.id, action, expectedRevision: order.revision }); await reload(); setNotice({ kind: 'success', text: `${order.orderNumber} was ${action === 'place' ? 'marked as placed' : 'cancelled'}. No supplier message was sent.` }) }
    catch (reason) { setNotice({ kind: 'error', text: reason instanceof Error ? reason.message : 'The order could not be updated.' }) }
    finally { setBusy(null) }
  }
  function startReceive(order: HostedPurchaseOrder) {
    setReceiving(order); setReceiptReference('')
    setReceiptLines(order.items.filter((line) => line.receivedQuantity < line.orderedQuantity).map((line) => ({ orderLineId: line.id, name: line.name, quantity: line.orderedQuantity - line.receivedQuantity, actualUnitCostMinor: line.expectedUnitCostMinor })))
    setNotice(null)
  }
  async function submitReceipt(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); if (!receiving) return
    const fingerprint = JSON.stringify({ reference: receiptReference.trim() || null, expectedRevision: receiving.revision, items: receiptLines })
    const attempt = receiptAttempt?.fingerprint === fingerprint ? receiptAttempt : { fingerprint, key: crypto.randomUUID() }
    setReceiptAttempt(attempt); setBusy(receiving.id); setNotice(null)
    try {
      await receiveBranchPurchaseOrder({ apiBaseUrl, accessToken, tenantId, branchId, orderId: receiving.id, idempotencyKey: attempt.key, expectedRevision: receiving.revision, reference: receiptReference.trim() || null, items: receiptLines.map(({ orderLineId, quantity, actualUnitCostMinor }) => ({ orderLineId, quantity, actualUnitCostMinor })) })
      setReceiptAttempt(null); setReceiving(null); await reload(); setNotice({ kind: 'success', text: 'Delivery received. Stock, movement history, cost snapshots and order status were updated.' })
    } catch (reason) { setNotice({ kind: 'error', text: `${reason instanceof Error ? reason.message : 'The delivery could not be recorded.'} Retry without changing the details to avoid duplicate stock.` }) }
    finally { setBusy(null) }
  }

  async function submitSupplierBill(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const totalMinor = parseMoneyInput(invoiceTotal)
    const allocations = Object.entries(receiptAllocations).flatMap(([receiptId, value]) => {
      const amountMinor = parseMoneyInput(value)
      return amountMinor !== null && amountMinor > 0 ? [{ receiptId, amountMinor }] : []
    })
    if (!billSupplierId || !billReference.trim() || totalMinor === null || allocations.length === 0) {
      setNotice({ kind: 'error', text: 'Enter the supplier invoice details and allocate at least one received delivery.' }); return
    }
    const payload = { supplierId: billSupplierId, supplierReference: billReference.trim(), invoiceDate, dueDate: dueDate || null, totalMinor, allocations }
    const fingerprint = JSON.stringify(payload)
    const attempt = billAttempt?.fingerprint === fingerprint ? billAttempt : { fingerprint, key: crypto.randomUUID() }
    setBillAttempt(attempt); setBusy('supplier-bill'); setNotice(null)
    try {
      const result = await matchBranchSupplierBill({ apiBaseUrl, accessToken, tenantId, branchId, idempotencyKey: attempt.key, ...payload })
      setBillAttempt(null); setBillReference(''); setDueDate(''); setInvoiceTotal(''); setReceiptAllocations({})
      await reload()
      setNotice({ kind: 'success', text: `Invoice ${result.bill.supplierReference} matched to received deliveries${result.replayed ? ' (retry confirmed)' : ''}.` })
    } catch (reason) {
      setNotice({ kind: 'error', text: `${reason instanceof Error ? reason.message : 'The supplier invoice could not be matched.'} Retry unchanged to avoid a duplicate.` })
    } finally { setBusy(null) }
  }

  async function submitSupplierPayment(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!billing || !paymentSupplierId) return
    const allocations = Object.entries(paymentAllocations).flatMap(([billId, value]) => {
      const amountMinor = parseMoneyInput(value)
      return amountMinor !== null && amountMinor > 0 ? [{ billId, amountMinor }] : []
    })
    const amountMinor = allocations.reduce((sum, item) => sum + item.amountMinor, 0)
    if (!allocations.length || !Number.isSafeInteger(amountMinor) || (paymentMethod === 'cash' && !paymentCashLocationId)) {
      setNotice({ kind: 'error', text: paymentMethod === 'cash' && !paymentCashLocationId ? 'Choose the initialized safe or petty-cash location used to pay the supplier.' : 'Allocate a valid amount to at least one open supplier invoice.' }); return
    }
    const payload = { supplierId: paymentSupplierId, paymentMethod, cashLocationId: paymentMethod === 'cash' ? paymentCashLocationId : null, externalReference: paymentReference.trim() || null, amountMinor, allocations }
    const fingerprint = JSON.stringify(payload)
    const attempt = paymentAttempt?.fingerprint === fingerprint ? paymentAttempt : { fingerprint, key: crypto.randomUUID() }
    setPaymentAttempt(attempt); setBusy('supplier-payment'); setNotice(null)
    try {
      const result = await recordBranchSupplierPayment({ apiBaseUrl, accessToken, tenantId, branchId, idempotencyKey: attempt.key, ...payload })
      setPaymentAttempt(null); setPaymentReference(''); setPaymentAllocations({})
      await reload()
      setNotice({ kind: 'success', text: `Supplier payment recorded for ${formatMinor(result.payment.amountMinor, result.payment.currencyCode)}${result.replayed ? ' (retry confirmed)' : ''}. This records a payment made outside the app; it does not contact or verify the provider.` })
    } catch (reason) {
      setNotice({ kind: 'error', text: `${reason instanceof Error ? reason.message : 'The supplier payment could not be recorded.'} Retry unchanged to avoid a duplicate.` })
    } finally { setBusy(null) }
  }

  async function submitSupplierCredit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!billing || !creditBillId) return
    const lines: HostedSupplierCreditLineInput[] = billing.returnableItems.flatMap((item) => {
      if (item.billId !== creditBillId) return []
      const raw = returnQuantities[item.receiptLineId] ?? ''
      if (!/^\d{1,7}$/.test(raw)) return []
      const quantity = Number(raw)
      return Number.isSafeInteger(quantity) && quantity > 0 && quantity <= item.maxQuantity
        ? [{ receiptId: item.receiptId, receiptLineId: item.receiptLineId, quantity }]
        : []
    })
    const hasInvalidQuantity = billing.returnableItems.some((item) => {
      if (item.billId !== creditBillId) return false
      const raw = returnQuantities[item.receiptLineId] ?? ''
      if (!raw) return false
      if (!/^\d{1,7}$/.test(raw)) return true
      const quantity = Number(raw)
      return !Number.isSafeInteger(quantity) || quantity < 0 || quantity > item.maxQuantity
    })
    const amountMinor = lines.reduce((sum, line) => {
      const item = billing.returnableItems.find((candidate) => candidate.receiptLineId === line.receiptLineId)
      return sum + (item ? item.unitCostMinor * line.quantity : 0)
    }, 0)
    const bill = billing.bills.find((item) => item.id === creditBillId)
    if (hasInvalidQuantity) {
      setNotice({ kind: 'error', text: 'Each return quantity must be a whole number within the available limit.' }); return
    }
    if (!bill || !creditReference.trim() || !creditReason.trim() || lines.length === 0 || !Number.isSafeInteger(amountMinor) || amountMinor <= 0) {
      setNotice({ kind: 'error', text: 'Choose an open invoice, enter a credit reference and reason, and set at least one valid return quantity.' }); return
    }
    if (amountMinor > bill.balanceMinor) {
      setNotice({ kind: 'error', text: 'The selected return value exceeds the invoice balance. Reduce the quantities or refresh the supplier billing data.' }); return
    }
    const payload = { billId: creditBillId, creditReference: creditReference.trim(), effectiveDate: creditDate, reason: creditReason.trim(), lines }
    const fingerprint = JSON.stringify(payload)
    const attempt = creditAttempt?.fingerprint === fingerprint ? creditAttempt : { fingerprint, key: crypto.randomUUID() }
    setCreditAttempt(attempt); setBusy('supplier-credit'); setNotice(null)
    try {
      const result = await createBranchSupplierCreditNote({ apiBaseUrl, accessToken, tenantId, branchId, idempotencyKey: attempt.key, ...payload })
      setCreditAttempt(null); setCreditReference(''); setCreditReason('Goods returned to supplier'); setReturnQuantities({})
      await reload()
      setNotice({ kind: 'success', text: `Credit ${result.creditNote.creditReference} recorded for ${formatMinor(result.creditNote.totalMinor, result.creditNote.currencyCode)}${result.replayed ? ' (retry confirmed)' : ''}. Returned stock and the unpaid invoice balance were updated.` })
    } catch (reason) {
      setNotice({ kind: 'error', text: `${reason instanceof Error ? reason.message : 'The supplier credit note could not be recorded.'} Retry unchanged to avoid a duplicate.` })
    } finally { setBusy(null) }
  }

  const payableBills = billing?.bills.filter((bill) => bill.supplierId === paymentSupplierId && bill.balanceMinor > 0) ?? []
  const paymentTotalMinor = Object.values(paymentAllocations).reduce((sum, value) => sum + (parseMoneyInput(value) ?? 0), 0)
  const returnableForBill = billing?.returnableItems.filter((item) => item.billId === creditBillId) ?? []
  const creditTotalMinor = returnableForBill.reduce((sum, item) => {
    const quantity = Number(returnQuantities[item.receiptLineId] || 0)
    return Number.isSafeInteger(quantity) && quantity > 0 && quantity <= item.maxQuantity ? sum + item.unitCostMinor * quantity : sum
  }, 0)

  const totalDraftMinor = lines.reduce((sum, line) => {
    const quantity = Number(line.quantity); const cost = parseMoneyInput(line.expectedCost)
    return Number.isSafeInteger(quantity) && quantity > 0 && cost !== null && Number.isSafeInteger(quantity * cost) ? sum + quantity * cost : sum
  }, 0)

  return <section className="hosted-purchases-panel" aria-labelledby="hosted-purchases-title">
    <header className="hosted-purchases-header"><div><p className="eyebrow">PURCHASING</p><h2 id="hosted-purchases-title">Purchase orders</h2><p>{branchName} · supplier contact and order delivery are managed separately.</p></div><span className="customer-directory-icon"><ClipboardList size={18}/></span></header>
    {canWrite && <div className="hosted-customer-create-toggle"><span>Save a draft, then mark it placed after sending it through your usual supplier channel.</span><button className="button button-primary" type="button" onClick={() => { setEditing(null); setFormOpen((open) => !open); setNotice(null) }} disabled={busy !== null}><Plus size={15}/>{formOpen && !editing ? 'Close draft' : 'New purchase order'}</button></div>}
    {formOpen && canWrite && <form className="hosted-customer-edit hosted-purchase-form" onSubmit={(event) => void saveDraft(event)}><div className="hosted-customer-edit-heading"><div><p className="eyebrow">{editing ? 'EDIT DRAFT' : 'NEW DRAFT'}</p><h3>{editing ? `Edit ${editing.orderNumber}` : 'Create purchase order'}</h3></div>{editing && <button type="button" className="button button-quiet" onClick={clearDraft} disabled={busy !== null}>Cancel edit</button>}</div>
      <div className="hosted-purchase-meta"><label>Supplier<select value={supplierId} onChange={(event) => setSupplierId(event.currentTarget.value)} disabled={busy !== null}><option value="">Unspecified supplier</option>{suppliers.map((supplier) => <option key={supplier.id} value={supplier.id}>{supplier.name}</option>)}</select></label><label>Supplier quote or reference<input maxLength={100} value={reference} onChange={(event) => setReference(event.currentTarget.value)} disabled={busy !== null} placeholder="Optional"/></label></div>
      <div className="hosted-purchase-line-heading"><strong>Products requested</strong><button type="button" className="button button-quiet button-small" onClick={() => { setLines((current) => [...current, newDraftLine(nextLineKey)]); setNextLineKey((value) => value + 1) }} disabled={busy !== null || lines.length >= 100}><Plus size={13}/> Add product</button></div>
      {lines.map((line, index) => <div className="hosted-purchase-line" key={line.key}><label>Product {index + 1}<select required value={line.productId} onChange={(event) => setLines((current) => current.map((item) => item.key === line.key ? { ...item, productId: event.currentTarget.value } : item))} disabled={busy !== null}><option value="">Select active product</option>{products.filter((product) => product.isActive).map((product) => <option value={product.productId} key={product.productId}>{product.name} · {product.sku} · {product.stockQuantity} in stock</option>)}</select></label><label>Quantity<input type="number" min="1" max="1000000" step="1" required value={line.quantity} onChange={(event) => setLines((current) => current.map((item) => item.key === line.key ? { ...item, quantity: event.currentTarget.value } : item))} disabled={busy !== null}/></label><label>Estimated unit cost ({currencyCode})<input inputMode="decimal" placeholder="0.00" required value={line.expectedCost} onChange={(event) => setLines((current) => current.map((item) => item.key === line.key ? { ...item, expectedCost: event.currentTarget.value } : item))} disabled={busy !== null}/></label><button type="button" className="button button-quiet button-small" aria-label={`Remove product ${index + 1}`} disabled={lines.length <= 1 || busy !== null} onClick={() => setLines((current) => current.filter((item) => item.key !== line.key))}>Remove</button></div>)}
      <div className="hosted-purchase-total"><span>Estimated order value</span><strong>{formatMinor(totalDraftMinor, currencyCode)}</strong></div><div className="hosted-purchase-form-actions"><button type="button" className="button button-quiet" onClick={clearDraft} disabled={busy !== null}>Cancel</button><button type="submit" className="button button-primary" disabled={busy !== null || products.length === 0}>{busy ? 'Saving…' : editing ? 'Save draft' : 'Save draft'}</button></div>
    </form>}
    {notice && <p className={`notice ${notice.kind === 'success' ? 'notice-success' : 'notice-danger'}`} role={notice.kind === 'error' ? 'alert' : 'status'}>{notice.text}</p>}
    {loading ? <p className="hosted-customer-empty" role="status">Loading purchase orders…</p> : orders.length === 0 ? <p className="hosted-customer-empty">No purchase orders yet. Drafts do not change stock; receiving a delivery posts inventory movements.</p> : <div className="hosted-purchase-list">{orders.map((order) => <article className="hosted-purchase-card" key={order.id}>
      <header><div><span className="eyebrow">{order.orderNumber}</span><h3>{order.supplierName || 'Unspecified supplier'}</h3><p>{order.reference || 'No supplier reference'} · Created {new Date(order.createdAt).toLocaleString()}</p></div><span className={`customer-status customer-status-${order.status === 'partially_received' ? 'active' : order.status === 'cancelled' ? 'archived' : 'active'}`}>{order.status.replaceAll('_', ' ')}</span></header>
      <div className="hosted-purchase-items">{order.items.map((line) => <div key={line.id}><span><strong>{line.name}</strong><small>{line.sku} · {line.receivedQuantity}/{line.orderedQuantity} received · estimated {formatMinor(line.expectedUnitCostMinor, currencyCode)} each</small></span><strong>{formatMinor(line.expectedUnitCostMinor * line.orderedQuantity, currencyCode)}</strong></div>)}</div>
      {order.receipts.length > 0 && <div className="hosted-purchase-receipts"><strong>Deliveries recorded</strong>{order.receipts.map((receipt) => <span key={receipt.id}>{receipt.reference || 'No invoice reference'} · {new Date(receipt.receivedAt).toLocaleString()} · {formatMinor(receipt.totalMinor, currencyCode)}</span>)}</div>}
      <footer>{canWrite && order.status === 'draft' && <><button type="button" className="button button-quiet button-small" onClick={() => startEdit(order)} disabled={busy !== null}><Pencil size={13}/> Edit draft</button><button type="button" className="button button-primary button-small" onClick={() => void transition(order, 'place')} disabled={busy !== null}><Check size={13}/> Mark placed</button><button type="button" className="button button-quiet button-small" onClick={() => void transition(order, 'cancel')} disabled={busy !== null}><Archive size={13}/> Cancel</button></>}{canWrite && ['placed','partially_received'].includes(order.status) && <button type="button" className="button button-quiet button-small" onClick={() => void transition(order, 'cancel')} disabled={busy !== null}><Archive size={13}/> Cancel remaining</button>}{canInvoice && ['placed','partially_received'].includes(order.status) && <button type="button" className="button button-primary button-small" onClick={() => startReceive(order)} disabled={busy !== null}><PackageCheck size={13}/> Receive delivery</button>}{busy === order.id && <span className="hosted-purchase-processing"><Truck size={13}/> Saving…</span>}</footer>
    </article>)}</div>}
    {canInvoice && <section className="hosted-billing-section" aria-labelledby="hosted-billing-title">
      <header className="hosted-purchases-header"><div><p className="eyebrow">SUPPLIER INVOICES</p><h2 id="hosted-billing-title">Match invoice to deliveries</h2><p>Allocate each invoice across one or more received deliveries from the same supplier.</p></div><FileCheck2 size={18}/></header>
      {billing && <form className="hosted-customer-edit hosted-purchase-form" onSubmit={(event) => void submitSupplierBill(event)}>
        <div className="hosted-purchase-meta">
          <label>Supplier<select required value={billSupplierId} onChange={(event) => { setBillSupplierId(event.currentTarget.value); setReceiptAllocations({}) }} disabled={busy !== null}><option value="">Choose supplier</option>{Array.from(new Map(billing.unbilledReceipts.map((receipt) => [receipt.supplierId, receipt.supplierName])).entries()).map(([id, name]) => <option key={id} value={id}>{name}</option>)}</select></label>
          <label>Supplier invoice number<input required maxLength={120} value={billReference} onChange={(event) => setBillReference(event.currentTarget.value)} disabled={busy !== null} placeholder="As printed on the invoice"/></label>
          <label>Invoice date<input required type="date" value={invoiceDate} onChange={(event) => setInvoiceDate(event.currentTarget.value)} disabled={busy !== null}/></label>
          <label>Due date<input type="date" min={invoiceDate} value={dueDate} onChange={(event) => setDueDate(event.currentTarget.value)} disabled={busy !== null}/></label>
          <label>Invoice total ({billing.currencyCode})<input required inputMode="decimal" placeholder="0.00" value={invoiceTotal} onChange={(event) => setInvoiceTotal(event.currentTarget.value)} disabled={busy !== null}/></label>
        </div>
        <div className="hosted-purchase-line-heading"><strong>Received deliveries to match</strong><span>Allocation amounts must add up to the invoice total.</span></div>
        {billing.unbilledReceipts.filter((receipt) => receipt.supplierId === billSupplierId).map((receipt) => <label className="hosted-bill-receipt" key={receipt.receiptId}>
          <span><strong>{receipt.supplierReference || 'Delivery without reference'}</strong><small>{new Date(receipt.receivedAt).toLocaleDateString()} · {formatMinor(receipt.billedMinor, billing.currencyCode)} already matched · {formatMinor(receipt.availableMinor, billing.currencyCode)} available</small></span>
          <span className="hosted-bill-allocation"><span>{billing.currencyCode}</span><input aria-label={`Amount to allocate from ${receipt.supplierReference || 'delivery'}`} inputMode="decimal" placeholder="0.00" value={receiptAllocations[receipt.receiptId] ?? ''} onChange={(event) => setReceiptAllocations((current) => ({ ...current, [receipt.receiptId]: event.currentTarget.value }))} disabled={busy !== null}/></span>
        </label>)}
        {billSupplierId && billing.unbilledReceipts.every((receipt) => receipt.supplierId !== billSupplierId) && <p className="hosted-customer-empty">No unbilled received deliveries are available for this supplier.</p>}
        <div className="hosted-purchase-form-actions"><button type="submit" className="button button-primary" disabled={busy !== null || !billSupplierId || billing.unbilledReceipts.length === 0}>{busy === 'supplier-bill' ? 'Matching invoice…' : 'Match supplier invoice'}</button></div>
      </form>}
      {billing && billing.bills.length > 0 && <div className="hosted-purchase-list"><h3>Matched invoices · accounts payable</h3>{billing.bills.map((bill) => <article className="hosted-purchase-card" key={bill.id}><header><div><span className="eyebrow">{bill.supplierReference}</span><h3>{bill.supplierName}</h3><p>Invoice {bill.invoiceDate}{bill.dueDate ? ` · Due ${bill.dueDate}` : ''} · Paid {formatMinor(bill.paidMinor, bill.currencyCode)} · Credited {formatMinor(bill.creditedMinor, bill.currencyCode)}</p></div><div><small>Open balance</small><strong>{formatMinor(bill.balanceMinor, bill.currencyCode)}</strong></div></header></article>)}</div>}
      {canSettle && billing && billing.bills.some((bill) => bill.balanceMinor > 0) && <form className="hosted-customer-edit hosted-purchase-form" onSubmit={(event) => void submitSupplierCredit(event)}>
        <div className="hosted-customer-edit-heading"><div><p className="eyebrow">SUPPLIER RETURNS</p><h3>Record a goods return</h3><p>Only returned goods still in branch stock can be credited. Credits use the original receipt cost and reduce the unpaid invoice balance.</p></div></div>
        <div className="hosted-purchase-meta">
          <label>Open invoice<select required value={creditBillId} onChange={(event) => { setCreditBillId(event.currentTarget.value); setReturnQuantities({}) }} disabled={busy !== null}><option value="">Choose invoice</option>{billing.bills.filter((bill) => bill.balanceMinor > 0).map((bill) => <option key={bill.id} value={bill.id}>{bill.supplierName} · {bill.supplierReference} · {formatMinor(bill.balanceMinor, bill.currencyCode)} open</option>)}</select></label>
          <label>Supplier credit note number<input required maxLength={120} value={creditReference} onChange={(event) => setCreditReference(event.currentTarget.value)} disabled={busy !== null} placeholder="As printed by the supplier"/></label>
          <label>Credit date<input required type="date" value={creditDate} onChange={(event) => setCreditDate(event.currentTarget.value)} disabled={busy !== null}/></label>
          <label>Reason<input required maxLength={500} value={creditReason} onChange={(event) => setCreditReason(event.currentTarget.value)} disabled={busy !== null}/></label>
        </div>
        <div className="hosted-purchase-line-heading"><strong>Received stock to return</strong><span>Maximum quantities account for earlier returns, current stock, commitments and this invoice's receipt allocation.</span></div>
        {returnableForBill.map((item) => <label className="hosted-bill-receipt" key={item.receiptLineId}>
          <span><strong>{item.productName} · {item.sku}</strong><small>{item.receiptReference || 'Delivery'} · original cost {formatMinor(item.unitCostMinor, billing.currencyCode)} · up to {item.maxQuantity}</small></span>
          <span className="hosted-bill-allocation"><span>Qty</span><input aria-label={`Quantity of ${item.productName} to return`} type="number" min="0" max={item.maxQuantity} step="1" value={returnQuantities[item.receiptLineId] ?? ''} onChange={(event) => setReturnQuantities((current) => ({ ...current, [item.receiptLineId]: event.currentTarget.value }))} disabled={busy !== null}/></span>
        </label>)}
        {creditBillId && returnableForBill.length === 0 && <p className="hosted-customer-empty">No returned quantity is currently available for this invoice. Stock may have been sold, reserved or already returned.</p>}
        <div className="hosted-purchase-total"><span>Supplier credit</span><strong>{formatMinor(creditTotalMinor, billing.currencyCode)}</strong></div>
        <div className="hosted-purchase-form-actions"><button type="submit" className="button button-primary" disabled={busy !== null || !creditBillId || creditTotalMinor <= 0}>{busy === 'supplier-credit' ? 'Recording credit…' : 'Record goods return credit'}</button></div>
      </form>}
      {billing && billing.creditNotes.length > 0 && <div className="hosted-purchase-list"><h3>Supplier credits recorded</h3>{billing.creditNotes.map((credit) => <article className="hosted-purchase-card" key={credit.id}><header><div><span className="eyebrow">{credit.creditReference}</span><h3>{credit.supplierName}</h3><p>{credit.effectiveDate} · {credit.reason} · Invoice {billing.bills.find((bill) => bill.id === credit.billId)?.supplierReference ?? 'supplier invoice'}</p></div><strong>{formatMinor(credit.totalMinor, credit.currencyCode)}</strong></header></article>)}</div>}
      {canSettle && billing && <form className="hosted-customer-edit hosted-purchase-form" onSubmit={(event) => void submitSupplierPayment(event)}>
        <div className="hosted-customer-edit-heading"><div><p className="eyebrow">SUPPLIER SETTLEMENT</p><h3>Record an external payment</h3><p>Only record a transfer or cash payment after it has been made through your usual channel.</p></div></div>
        <div className="hosted-purchase-meta">
          <label>Supplier<select required value={paymentSupplierId} onChange={(event) => { setPaymentSupplierId(event.currentTarget.value); setPaymentAllocations({}) }} disabled={busy !== null}><option value="">Choose supplier</option>{Array.from(new Map(billing.bills.filter((bill) => bill.balanceMinor > 0).map((bill) => [bill.supplierId, bill.supplierName])).entries()).map(([id, name]) => <option key={id} value={id}>{name}</option>)}</select></label>
          <label>Payment method<select value={paymentMethod} onChange={(event) => { const value = event.currentTarget.value; if (isSupplierPaymentMethod(value)) { setPaymentMethod(value); if (value !== 'cash') setPaymentCashLocationId('') } }} disabled={busy !== null}><option value="bank_transfer">Bank transfer</option><option value="mobile_money">Mobile money</option><option value="card">Card</option><option value="other">Other</option><option value="cash">Safe / petty cash</option></select></label>
          {paymentMethod === 'cash' && <label>Paid from<select required value={paymentCashLocationId} onChange={(event) => setPaymentCashLocationId(event.currentTarget.value)} disabled={busy !== null}><option value="">Choose initialized safe / petty cash</option>{cashLocations.map((location) => <option key={location.id} value={location.id}>{location.name} · {formatMinor(location.balanceMinor, location.currencyCode)}</option>)}</select></label>}
          <label>Payment reference<input maxLength={100} value={paymentReference} onChange={(event) => setPaymentReference(event.currentTarget.value)} disabled={busy !== null} placeholder="Optional bank or receipt reference"/></label>
        </div>
        <div className="hosted-purchase-line-heading"><strong>Allocate to open invoices</strong><span>The payment is allocated in full; partial invoice settlement is supported. Cash here is paid from a selected safe or petty-cash location. Till payments use the signed station drawer controls.</span></div>
        {payableBills.map((bill) => <label className="hosted-bill-receipt" key={bill.id}>
          <span><strong>{bill.supplierReference}</strong><small>Invoice {bill.invoiceDate} · {formatMinor(bill.balanceMinor, bill.currencyCode)} remaining</small></span>
          <span className="hosted-bill-allocation"><span>{billing.currencyCode}</span><input aria-label={`Amount to allocate to invoice ${bill.supplierReference}`} inputMode="decimal" placeholder="0.00" value={paymentAllocations[bill.id] ?? ''} onChange={(event) => setPaymentAllocations((current) => ({ ...current, [bill.id]: event.currentTarget.value }))} disabled={busy !== null}/></span>
        </label>)}
        {paymentSupplierId && payableBills.length === 0 && <p className="hosted-customer-empty">This supplier has no open invoices to settle.</p>}
        <div className="hosted-purchase-total"><span>Payment total</span><strong>{formatMinor(paymentTotalMinor, billing.currencyCode)}</strong></div>
        <div className="hosted-purchase-form-actions"><button type="submit" className="button button-primary" disabled={busy !== null || !paymentSupplierId || paymentTotalMinor <= 0}>{busy === 'supplier-payment' ? 'Recording payment…' : 'Record payment'}</button></div>
      </form>}
      {billing && billing.payments.length > 0 && <div className="hosted-purchase-list"><h3>Payments recorded</h3>{billing.payments.map((payment) => <article className="hosted-purchase-card" key={payment.id}><header><div><span className="eyebrow">{payment.paymentMethod.replaceAll('_', ' ')}</span><h3>{payment.supplierName}</h3><p>{new Date(payment.paidAt).toLocaleString()}{payment.cashLocationId ? ` · ${cashLocations.find((location) => location.id === payment.cashLocationId)?.name ?? 'Cash location'}` : ''}{payment.externalReference ? ` · Reference ${payment.externalReference}` : ''}</p></div><strong>{formatMinor(payment.amountMinor, payment.currencyCode)}</strong></header></article>)}</div>}
      <p className="hosted-purchase-disclosure">Invoice matches and payments create immutable, balanced source journal entries. External transfers are tenant-reported and not provider-verified. Safe/petty-cash payments reduce the selected tracked location; till payments remain linked to a signed station drawer. Complete financial statement reconciliation remains in progress.</p>
    </section>}
    {receiving && <form className="hosted-customer-edit hosted-purchase-form" onSubmit={(event) => void submitReceipt(event)}><div className="hosted-customer-edit-heading"><div><p className="eyebrow">INVENTORY RECEIPT</p><h3>Receive {receiving.orderNumber}</h3><p>Only entered quantities will be posted. Unreceived quantities stay open.</p></div><button type="button" className="button button-quiet" onClick={() => setReceiving(null)} disabled={busy !== null}>Close</button></div><label className="hosted-purchase-reference">Invoice or delivery reference<input maxLength={100} value={receiptReference} onChange={(event) => setReceiptReference(event.currentTarget.value)} disabled={busy !== null}/></label>
      {receiptLines.map((line,index) => {const orderLine=receiving.items.find((item)=>item.id===line.orderLineId)!;const remaining=orderLine.orderedQuantity-orderLine.receivedQuantity;return <div className="hosted-purchase-line" key={line.orderLineId}><label>Product {index+1}<span>{line.name}<small>{remaining} remaining on order</small></span></label><label>Received<input type="number" min="0" max={remaining} step="1" value={line.quantity} onChange={(event)=>setReceiptLines((current)=>current.map((item)=>item.orderLineId===line.orderLineId?{...item,quantity:Number(event.currentTarget.value)}:item))} disabled={busy!==null}/></label><label>Actual unit cost ({currencyCode})<input inputMode="decimal" value={moneyInput(line.actualUnitCostMinor)} onChange={(event)=>{const amount=parseMoneyInput(event.currentTarget.value);if(amount!==null)setReceiptLines((current)=>current.map((item)=>item.orderLineId===line.orderLineId?{...item,actualUnitCostMinor:amount}:item))}} disabled={busy!==null}/></label></div>})}
      <div className="hosted-purchase-form-actions"><button type="button" className="button button-quiet" onClick={()=>setReceiving(null)} disabled={busy!==null}>Cancel</button><button type="submit" className="button button-primary" disabled={busy!==null||receiptLines.every((line)=>line.quantity<=0)}>{busy===receiving.id?'Receiving…':'Record delivery'}</button></div>
    </form>}
    <p className="hosted-purchase-disclosure">The order is sent to the supplier outside this app. A receipt updates branch stock and the last recorded unit cost.</p>
  </section>
}
