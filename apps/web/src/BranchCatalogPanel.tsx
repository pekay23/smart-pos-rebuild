// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { Fragment, useCallback, useEffect, useState, type FormEvent } from 'react'
import { Archive, Barcode, Boxes, Check, ChevronDown, CircleAlert, PackagePlus, RefreshCw, RotateCcw, Save, Search } from 'lucide-react'
import {
  ApiError,
  createBranchProduct,
  attachBranchCatalogProduct,
  listBranchCatalogCandidates,
  listBranchProducts,
  setBranchProductActive,
  updateBranchProductSettings,
  updateProductIdentity,
} from './api'
import type { BranchProduct, CatalogProductIdentity, NewBranchProduct } from './api'
import { HostedEntityImage } from './HostedEntityImage'

type CatalogLoad = { kind: 'loading' } | { kind: 'ready'; products: BranchProduct[] } | { kind: 'error'; message: string }
type CatalogNotice = { kind: 'success' | 'error'; message: string }
type CandidateLoad = { kind: 'loading' } | { kind: 'ready'; products: CatalogProductIdentity[] } | { kind: 'error'; message: string }
type NewProductDraft = {
  sku: string; barcode: string; name: string; category: string; price: string; taxPercent: string
  openingStock: string; reorderPoint: string; targetStock: string
}
type ProductDraft = {
  sku: string; barcode: string; name: string; description: string; category: string
  price: string; taxPercent: string; reorderPoint: string; targetStock: string
}

const EMPTY_DRAFT: NewProductDraft = {
  sku: '', barcode: '', name: '', category: '', price: '', taxPercent: '0', openingStock: '0', reorderPoint: '0', targetStock: '0',
}

function errorMessage(reason: unknown, fallback: string): string {
  return reason instanceof Error ? reason.message : fallback
}

function integerInput(value: string, label: string): number {
  if (!/^\d+$/.test(value)) throw new Error(`${label} must be a whole number.`)
  const parsed = Number(value)
  if (!Number.isSafeInteger(parsed)) throw new Error(`${label} is outside the supported range.`)
  return parsed
}

function moneyInput(value: string): number {
  if (!/^\d+(\.\d{1,2})?$/.test(value)) throw new Error('Enter a valid price with up to two decimal places.')
  const [whole, fraction = ''] = value.split('.')
  const amount = Number(whole) * 100 + Number(fraction.padEnd(2, '0'))
  if (!Number.isSafeInteger(amount)) throw new Error('Price is outside the supported range.')
  return amount
}

function percentInput(value: string): number {
  if (!/^\d{1,2}(\.\d{1,2})?$/.test(value)) throw new Error('Enter a tax rate from 0 to 100 percent.')
  const [whole, fraction = ''] = value.split('.')
  const basisPoints = Number(whole) * 100 + Number(fraction.padEnd(2, '0'))
  if (basisPoints > 10000) throw new Error('Tax rate cannot exceed 100 percent.')
  return basisPoints
}

function productDraft(product: BranchProduct): ProductDraft {
  return {
    sku: product.sku,
    barcode: product.barcode ?? '',
    name: product.name,
    description: product.description ?? '',
    category: product.category ?? '',
    price: (product.priceMinor / 100).toFixed(2),
    taxPercent: (product.taxRateBps / 100).toFixed(2),
    reorderPoint: String(product.reorderPoint),
    targetStock: String(product.targetStockQuantity),
  }
}

export function BranchCatalogPanel({ apiBaseUrl, accessToken, tenantId, branchId, branchName, currencyCode, canManageBranch, canManageIdentity }: {
  apiBaseUrl: string
  accessToken: string
  tenantId: string
  branchId: string
  branchName: string
  currencyCode: string
  canManageBranch: boolean
  canManageIdentity: boolean
}) {
  const [catalog, setCatalog] = useState<CatalogLoad>({ kind: 'loading' })
  const [notice, setNotice] = useState<CatalogNotice | null>(null)
  const [query, setQuery] = useState('')
  const [createOpen, setCreateOpen] = useState(false)
  const [addMode, setAddMode] = useState<'new' | 'existing'>('new')
  const [newProduct, setNewProduct] = useState(EMPTY_DRAFT)
  const [candidateState, setCandidateState] = useState<CandidateLoad | null>(null)
  const [candidateQuery, setCandidateQuery] = useState('')
  const [selectedCandidateId, setSelectedCandidateId] = useState('')
  const [attachPrice, setAttachPrice] = useState('')
  const [attachTax, setAttachTax] = useState('0')
  const [attachStock, setAttachStock] = useState('0')
  const [attachReorder, setAttachReorder] = useState('0')
  const [attachTarget, setAttachTarget] = useState('0')
  const [editingProductId, setEditingProductId] = useState<string | null>(null)
  const [draft, setDraft] = useState<ProductDraft | null>(null)
  const [busyProductId, setBusyProductId] = useState<string | null>(null)
  const [creating, setCreating] = useState(false)
  const [attaching, setAttaching] = useState(false)

  const loadCatalog = useCallback(async () => {
    setCatalog({ kind: 'loading' })
    try {
      const products = await listBranchProducts({ apiBaseUrl, accessToken, tenantId, branchId })
      setCatalog({ kind: 'ready', products })
    } catch (reason) {
      setCatalog({ kind: 'error', message: errorMessage(reason, 'The branch catalog could not be loaded.') })
    }
  }, [accessToken, apiBaseUrl, branchId, tenantId])

  useEffect(() => { void loadCatalog() }, [loadCatalog])

  const visibleProducts = catalog.kind === 'ready'
    ? catalog.products.filter((product) => `${product.name} ${product.sku} ${product.barcode ?? ''} ${product.category ?? ''}`.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()))
    : []
  const visibleCandidates = candidateState?.kind === 'ready'
    ? candidateState.products.filter((product) => `${product.name} ${product.sku} ${product.barcode ?? ''} ${product.category ?? ''}`.toLocaleLowerCase().includes(candidateQuery.trim().toLocaleLowerCase()))
    : []

  function replaceProduct(updated: BranchProduct) {
    setCatalog((current) => current.kind === 'ready'
      ? { kind: 'ready', products: current.products.map((product) => product.productId === updated.productId ? updated : product) }
      : current)
  }

  async function saveNewProduct(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setCreating(true)
    setNotice(null)
    try {
      const payload: NewBranchProduct = {
        sku: newProduct.sku.trim(),
        barcode: newProduct.barcode.trim() || null,
        name: newProduct.name.trim(),
        description: null,
        category: newProduct.category.trim() || null,
        priceMinor: moneyInput(newProduct.price),
        costMinor: null,
        taxRateBps: percentInput(newProduct.taxPercent),
        openingStock: integerInput(newProduct.openingStock, 'Opening stock'),
        reorderPoint: integerInput(newProduct.reorderPoint, 'Reorder point'),
        targetStockQuantity: integerInput(newProduct.targetStock, 'Target stock'),
      }
      const product = await createBranchProduct({ apiBaseUrl, accessToken, tenantId, branchId, product: payload })
      setCatalog((current) => current.kind === 'ready'
        ? { kind: 'ready', products: [...current.products, product].sort((a, b) => a.name.localeCompare(b.name)) }
        : current)
      setNewProduct(EMPTY_DRAFT)
      setCreateOpen(false)
      setNotice({ kind: 'success', message: `${product.name} was added to ${branchName}.` })
    } catch (reason) {
      setNotice({ kind: 'error', message: errorMessage(reason, 'The product could not be added.') })
    } finally {
      setCreating(false)
    }
  }

  async function loadProductCandidates() {
    setCandidateState({ kind: 'loading' })
    try {
      const products = await listBranchCatalogCandidates({ apiBaseUrl, accessToken, tenantId, branchId })
      setCandidateState({ kind: 'ready', products })
    } catch (reason) {
      setCandidateState({ kind: 'error', message: errorMessage(reason, 'Other business products could not be loaded.') })
    }
  }

  async function attachExistingProduct(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const candidate = candidateState?.kind === 'ready' ? candidateState.products.find((product) => product.productId === selectedCandidateId) : undefined
    if (!candidate) {
      setNotice({ kind: 'error', message: 'Select an existing business product before adding it to this branch.' })
      return
    }
    setAttaching(true)
    setNotice(null)
    try {
      const product = await attachBranchCatalogProduct({
        apiBaseUrl, accessToken, tenantId, branchId, productId: candidate.productId,
        settings: {
          priceMinor: moneyInput(attachPrice), costMinor: null, taxRateBps: percentInput(attachTax),
          openingStock: integerInput(attachStock, 'Opening stock'),
          reorderPoint: integerInput(attachReorder, 'Reorder point'),
          targetStockQuantity: integerInput(attachTarget, 'Target stock'),
        },
      })
      setCatalog((current) => current.kind === 'ready'
        ? { kind: 'ready', products: [...current.products, product].sort((a, b) => a.name.localeCompare(b.name)) }
        : current)
      setCandidateState((current) => current?.kind === 'ready'
        ? { kind: 'ready', products: current.products.filter((entry) => entry.productId !== product.productId) }
        : current)
      setSelectedCandidateId('')
      setAttachPrice(''); setAttachTax('0'); setAttachStock('0'); setAttachReorder('0'); setAttachTarget('0')
      setCreateOpen(false)
      setNotice({ kind: 'success', message: `${product.name} (${product.sku}) was added to ${branchName} with separate branch stock and pricing.` })
    } catch (reason) {
      setNotice({ kind: 'error', message: errorMessage(reason, 'The existing product could not be added to this branch.') })
    } finally {
      setAttaching(false)
    }
  }

  async function saveBranchSettings(product: BranchProduct) {
    if (!draft) return
    setBusyProductId(product.productId)
    setNotice(null)
    try {
      const updated = await updateBranchProductSettings({
        apiBaseUrl, accessToken, tenantId, branchId, productId: product.productId,
        priceMinor: moneyInput(draft.price),
        taxRateBps: percentInput(draft.taxPercent),
        reorderPoint: integerInput(draft.reorderPoint, 'Reorder point'),
        targetStockQuantity: integerInput(draft.targetStock, 'Target stock'),
        expectedRevision: product.revision,
      })
      replaceProduct(updated)
      setNotice({ kind: 'success', message: `${product.name} branch settings saved.` })
    } catch (reason) {
      setNotice({ kind: 'error', message: reason instanceof ApiError && reason.status === 409
        ? 'This product changed in another session. Refresh the catalog before editing again.'
        : errorMessage(reason, 'Branch settings could not be saved.') })
      if (reason instanceof ApiError && reason.status === 409) void loadCatalog()
    } finally {
      setBusyProductId(null)
    }
  }

  async function saveIdentity(product: BranchProduct) {
    if (!draft) return
    setBusyProductId(product.productId)
    setNotice(null)
    try {
      await updateProductIdentity({
        apiBaseUrl, accessToken, tenantId, productId: product.productId,
        sku: draft.sku.trim(), barcode: draft.barcode.trim() || null,
        name: draft.name.trim(), description: draft.description.trim() || null,
        category: draft.category.trim() || null, expectedRevision: product.identityRevision,
      })
      setNotice({ kind: 'success', message: 'Shared product details were updated across this business.' })
      await loadCatalog()
    } catch (reason) {
      setNotice({ kind: 'error', message: reason instanceof ApiError && reason.status === 409
        ? 'Shared product details changed in another session or this SKU/barcode is already used. Refresh and retry.'
        : errorMessage(reason, 'Shared product details could not be saved.') })
      if (reason instanceof ApiError && reason.status === 409) void loadCatalog()
    } finally {
      setBusyProductId(null)
    }
  }

  async function toggleActive(product: BranchProduct) {
    setBusyProductId(product.productId)
    setNotice(null)
    try {
      const updated = await setBranchProductActive({
        apiBaseUrl, accessToken, tenantId, branchId, productId: product.productId, isActive: !product.isActive,
      })
      replaceProduct(updated)
      setNotice({ kind: 'success', message: `${product.name} was ${updated.isActive ? 'restored' : 'archived'} at ${branchName}.` })
    } catch (reason) {
      setNotice({ kind: 'error', message: errorMessage(reason, 'Product status could not be changed.') })
    } finally {
      setBusyProductId(null)
    }
  }

  return <section className="catalog-panel" aria-labelledby="catalog-title">
    <header className="catalog-header">
      <div className="catalog-title-group"><span className="catalog-icon"><Boxes size={18}/></span><div><p className="eyebrow">BRANCH CATALOG</p><h2 id="catalog-title">Products at {branchName}</h2><p>Prices and stock alerts are branch-specific. Stock changes are recorded as inventory movements.</p></div></div>
      <div className="catalog-header-actions"><button type="button" className="button button-quiet catalog-refresh" onClick={() => void loadCatalog()} disabled={catalog.kind === 'loading'}><RefreshCw size={15}/> Refresh</button>{canManageBranch && <button type="button" className="button button-primary" onClick={() => setCreateOpen((open) => !open)}><PackagePlus size={16}/> Add product</button>}</div>
    </header>

    {notice && <div className={`catalog-notice ${notice.kind}`} role={notice.kind === 'error' ? 'alert' : 'status'}><span>{notice.kind === 'error' ? <CircleAlert size={16}/> : <Check size={16}/>}</span>{notice.message}<button type="button" aria-label="Dismiss message" onClick={() => setNotice(null)}>×</button></div>}

    {createOpen && canManageBranch && <div className="catalog-create-form">
      <div className="catalog-form-mode"><button type="button" className={`button ${addMode === 'new' ? 'button-primary' : 'button-quiet'}`} onClick={() => setAddMode('new')}>Create new product</button><button type="button" className={`button ${addMode === 'existing' ? 'button-primary' : 'button-quiet'}`} onClick={() => { setAddMode('existing'); if (candidateState === null) void loadProductCandidates() }}>Add existing business product</button></div>
      {addMode === 'new' && <form onSubmit={(event) => void saveNewProduct(event)}>
      <div className="catalog-form-heading"><div><p className="eyebrow">NEW ITEM</p><h3>Add a product to this branch</h3></div><span>Blank SKU is generated automatically</span></div>
      <label>Product name<input required maxLength={160} autoFocus value={newProduct.name} onChange={(event) => setNewProduct((current) => ({ ...current, name: event.target.value }))}/></label>
      <div className="catalog-form-grid">
        <label>SKU<input maxLength={64} value={newProduct.sku} onChange={(event) => setNewProduct((current) => ({ ...current, sku: event.target.value }))} placeholder="Generate automatically"/></label>
        <label>Barcode<input maxLength={64} value={newProduct.barcode} onChange={(event) => setNewProduct((current) => ({ ...current, barcode: event.target.value }))} placeholder="Optional"/></label>
        <label>Category<input maxLength={120} value={newProduct.category} onChange={(event) => setNewProduct((current) => ({ ...current, category: event.target.value }))} placeholder="Optional"/></label>
        <label>Price ({currencyCode})<input required inputMode="decimal" value={newProduct.price} onChange={(event) => setNewProduct((current) => ({ ...current, price: event.target.value }))} placeholder="0.00"/></label>
        <label>Tax (%)<input required inputMode="decimal" value={newProduct.taxPercent} onChange={(event) => setNewProduct((current) => ({ ...current, taxPercent: event.target.value }))}/></label>
        <label>Opening stock<input required inputMode="numeric" value={newProduct.openingStock} onChange={(event) => setNewProduct((current) => ({ ...current, openingStock: event.target.value }))}/></label>
        <label>Reorder point<input required inputMode="numeric" value={newProduct.reorderPoint} onChange={(event) => setNewProduct((current) => ({ ...current, reorderPoint: event.target.value }))}/></label>
        <label>Target stock<input required inputMode="numeric" value={newProduct.targetStock} onChange={(event) => setNewProduct((current) => ({ ...current, targetStock: event.target.value }))}/></label>
      </div>
      <div className="catalog-form-actions"><button type="button" className="button button-quiet" onClick={() => { setCreateOpen(false); setNewProduct(EMPTY_DRAFT) }} disabled={creating}>Cancel</button><button type="submit" className="button button-primary" disabled={creating}>{creating ? 'Saving…' : 'Save product'}</button></div>
      </form>}
      {addMode === 'existing' && <section className="catalog-existing-products" aria-label="Add a product already in this business">
        <div className="catalog-form-heading"><div><p className="eyebrow">SHARED PRODUCT</p><h3>Add an existing business product</h3></div><button type="button" className="button button-quiet button-small" onClick={() => void loadProductCandidates()} disabled={candidateState?.kind === 'loading'}><RefreshCw size={14}/> Refresh list</button></div>
        <p>Choose the exact business product by its SKU or barcode. This adds a branch-specific price and stock record; it does not merge product identities or change another branch.</p>
        {candidateState?.kind === 'loading' && <div className="catalog-state"><span className="mini-spinner"/>Loading business products…</div>}
        {candidateState?.kind === 'error' && <div className="catalog-state catalog-state-error" role="alert"><CircleAlert size={16}/><span>{candidateState.message}</span><button type="button" className="button button-quiet" onClick={() => void loadProductCandidates()}>Try again</button></div>}
        {candidateState?.kind === 'ready' && <>
          <label className="catalog-existing-search">Search name, SKU, barcode or category<input type="search" value={candidateQuery} onChange={(event) => setCandidateQuery(event.target.value)}/></label>
          {candidateState.products.length === 0 ? <p className="catalog-state">No active business products are waiting to be added to this branch.</p> : visibleCandidates.length === 0 ? <p className="catalog-state">No business products match that search.</p> : <div className="catalog-candidate-list">{visibleCandidates.map((candidate) => <button type="button" className={`catalog-candidate${selectedCandidateId === candidate.productId ? ' selected' : ''}`} key={candidate.productId} onClick={() => { setSelectedCandidateId(candidate.productId); setAttachPrice('') }}><strong>{candidate.name}</strong><span>SKU {candidate.sku}{candidate.barcode ? ` · Barcode ${candidate.barcode}` : ''}</span><small>{candidate.category ?? 'Uncategorized'}</small></button>)}</div>}
          {selectedCandidateId && candidateState.products.some((product) => product.productId === selectedCandidateId) && <form className="catalog-attach-form" onSubmit={(event) => void attachExistingProduct(event)}>
            <h4>Branch setup for {candidateState.products.find((product) => product.productId === selectedCandidateId)?.name}</h4>
            <div className="catalog-form-grid">
              <label>Price ({currencyCode})<input required inputMode="decimal" value={attachPrice} onChange={(event) => setAttachPrice(event.target.value)} placeholder="0.00"/></label>
              <label>Tax (%)<input required inputMode="decimal" value={attachTax} onChange={(event) => setAttachTax(event.target.value)}/></label>
              <label>Opening stock<input required inputMode="numeric" value={attachStock} onChange={(event) => setAttachStock(event.target.value)}/></label>
              <label>Reorder point<input required inputMode="numeric" value={attachReorder} onChange={(event) => setAttachReorder(event.target.value)}/></label>
              <label>Target stock<input required inputMode="numeric" value={attachTarget} onChange={(event) => setAttachTarget(event.target.value)}/></label>
            </div>
            <div className="catalog-form-actions"><button type="button" className="button button-quiet" onClick={() => { setCreateOpen(false); setSelectedCandidateId('') }} disabled={attaching}>Cancel</button><button type="submit" className="button button-primary" disabled={attaching}>{attaching ? 'Adding…' : 'Add product to branch'}</button></div>
          </form>}
        </>}
      </section>}
    </div>}

    <div className="catalog-toolbar"><label className="catalog-search"><Search size={16}/><input type="search" value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search name, SKU, barcode or category" aria-label="Search branch products"/></label>{catalog.kind === 'ready' && <span className="catalog-count">{visibleProducts.length} of {catalog.products.length} products</span>}</div>

    {catalog.kind === 'loading' && <div className="catalog-state"><span className="mini-spinner"/><span>Loading branch catalog…</span></div>}
    {catalog.kind === 'error' && <div className="catalog-state catalog-state-error"><CircleAlert size={18}/><span>{catalog.message}</span><button type="button" className="button button-quiet" onClick={() => void loadCatalog()}>Try again</button></div>}
    {catalog.kind === 'ready' && catalog.products.length === 0 && <div className="catalog-state catalog-empty"><span className="empty-icon"><PackagePlus size={20}/></span><strong>No products in this branch yet</strong><span>{canManageBranch ? 'Add the first item to start building this branch catalog.' : 'A manager can add products to this branch.'}</span></div>}
    {catalog.kind === 'ready' && catalog.products.length > 0 && visibleProducts.length === 0 && <div className="catalog-state">No products match “{query}”.</div>}
    {catalog.kind === 'ready' && visibleProducts.length > 0 && <div className="catalog-table-wrap"><table className="catalog-table"><thead><tr><th>PRODUCT</th><th>SKU / BARCODE</th><th>PRICE</th><th>ON HAND</th><th>STATUS</th><th><span className="sr-only">Actions</span></th></tr></thead><tbody>{visibleProducts.map((product) => {
      const editing = editingProductId === product.productId && draft !== null
      const busy = busyProductId === product.productId
      return <Fragment key={product.productId}>
      <tr className={!product.isActive ? 'catalog-row-inactive' : ''}>
        <td><div className="hosted-record-name"><HostedEntityImage apiBaseUrl={apiBaseUrl} accessToken={accessToken} tenantId={tenantId} branchId={branchId} entityType="product" entityId={product.productId} label={product.name} canEdit={canManageIdentity}/><span><strong>{product.name}</strong><small>{product.category || 'Uncategorized'}</small></span></div></td>
        <td><span className="catalog-code">{product.sku}</span><small>{product.barcode ? <><Barcode size={12}/>{product.barcode}</> : 'No barcode'}</small></td>
        <td><strong className="catalog-money">{product.currencyCode} {(product.priceMinor / 100).toFixed(2)}</strong><small>{(product.taxRateBps / 100).toFixed(2)}% tax</small></td>
        <td><strong className={product.stockQuantity <= product.reorderPoint ? 'stock-low' : ''}>{product.stockQuantity}</strong><small>Reorder at {product.reorderPoint}</small></td>
        <td><span className={`catalog-status ${product.isActive ? 'active' : 'inactive'}`}>{product.isActive ? 'Active' : 'Archived'}</span></td>
        <td><details className="catalog-row-actions"><summary aria-label={`Manage ${product.name}`}><ChevronDown size={16}/></summary><div className="catalog-action-menu">
          {canManageBranch && <button type="button" onClick={() => { setEditingProductId(editing ? null : product.productId); setDraft(editing ? null : productDraft(product)) }}><Save size={14}/>{editing ? 'Close editor' : 'Edit branch details'}</button>}
          {canManageBranch && <button type="button" disabled={busy} onClick={() => void toggleActive(product)}>{product.isActive ? <Archive size={14}/> : <RotateCcw size={14}/>} {product.isActive ? 'Archive at this branch' : 'Restore at this branch'}</button>}
        </div></details></td>
      </tr>
      {editing && draft && <tr><td colSpan={6} className="catalog-editor-cell"><div className="catalog-editor-grid">
          {canManageIdentity && <fieldset><legend>Shared details · applies to every branch</legend><div className="catalog-form-grid">
            <label>Product name<input maxLength={160} value={draft.name} onChange={(event) => setDraft({ ...draft, name: event.target.value })}/></label>
            <label>SKU<input maxLength={64} value={draft.sku} onChange={(event) => setDraft({ ...draft, sku: event.target.value })}/></label>
            <label>Barcode<input maxLength={64} value={draft.barcode} onChange={(event) => setDraft({ ...draft, barcode: event.target.value })}/></label>
            <label>Category<input maxLength={120} value={draft.category} onChange={(event) => setDraft({ ...draft, category: event.target.value })}/></label>
            <label className="catalog-span-two">Description<input maxLength={2000} value={draft.description} onChange={(event) => setDraft({ ...draft, description: event.target.value })}/></label>
          </div><button type="button" className="button button-quiet" disabled={busy} onClick={() => void saveIdentity(product)}>{busy ? 'Saving…' : 'Save shared details'}</button></fieldset>}
          {canManageBranch && <fieldset><legend>Branch details · {branchName}</legend><div className="catalog-form-grid">
            <label>Price ({product.currencyCode})<input inputMode="decimal" value={draft.price} onChange={(event) => setDraft({ ...draft, price: event.target.value })}/></label>
            <label>Tax (%)<input inputMode="decimal" value={draft.taxPercent} onChange={(event) => setDraft({ ...draft, taxPercent: event.target.value })}/></label>
            <label>Reorder point<input inputMode="numeric" value={draft.reorderPoint} onChange={(event) => setDraft({ ...draft, reorderPoint: event.target.value })}/></label>
            <label>Target stock<input inputMode="numeric" value={draft.targetStock} onChange={(event) => setDraft({ ...draft, targetStock: event.target.value })}/></label>
          </div><p className="catalog-stock-note"><Boxes size={13}/> On-hand quantity is changed through a stock adjustment or transfer.</p><button type="button" className="button button-primary" disabled={busy} onClick={() => void saveBranchSettings(product)}>{busy ? 'Saving…' : 'Save branch details'}</button></fieldset>}
          {!canManageIdentity && !canManageBranch && <p>Catalog edit permission is required.</p>}
        </div></td></tr>}
      </Fragment>
    })}</tbody></table></div>}
  </section>
}
