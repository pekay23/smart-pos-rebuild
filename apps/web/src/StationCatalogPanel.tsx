import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Barcode, Boxes, CircleAlert, Clock3, Mail, MapPin, Phone, RefreshCw, Search, Store } from 'lucide-react'
import { getProductBranchAvailability, listStationProducts } from './api'
import type { BranchProduct, ProductBranchAvailability } from './api'
import { formatObservedAt, isAvailabilityStale } from './availabilityFreshness'

type LoadState = { kind: 'loading' } | { kind: 'ready'; products: BranchProduct[] } | { kind: 'error'; message: string }
type AvailabilityState = { kind: 'loading' } | { kind: 'ready'; branches: ProductBranchAvailability[] } | { kind: 'error'; message: string }

function messageOf(reason: unknown, fallback: string): string {
  return reason instanceof Error ? reason.message : fallback
}

function formatMoney(currencyCode: string, amountMinor: number): string {
  return `${currencyCode} ${(amountMinor / 100).toFixed(2)}`
}

function branchOpeningStatus(branch: ProductBranchAvailability): string {
  const weekdays = ['sunday', 'monday', 'tuesday', 'wednesday', 'thursday', 'friday', 'saturday'] as const
  const timeLabel = (value: string) => value
  const allClosed = weekdays.every((day) => branch.openingHours[day].length === 0)
  if (allClosed) return 'Hours not posted'
  try {
    const parts = new Intl.DateTimeFormat('en-GB', {
      timeZone: branch.timeZone, weekday: 'long', hour: '2-digit', minute: '2-digit', hourCycle: 'h23',
    }).formatToParts(new Date(branch.observedAt))
    const part = (type: Intl.DateTimeFormatPartTypes) => parts.find((value) => value.type === type)?.value ?? ''
    const today = part('weekday').toLowerCase() as typeof weekdays[number]
    const minutesNow = Number(part('hour')) * 60 + Number(part('minute'))
    const todayHours = branch.openingHours[today] ?? []
    const active = todayHours.find((interval) => minutesNow >= Number(interval.open.slice(0, 2)) * 60 + Number(interval.open.slice(3))
      && minutesNow < Number(interval.close.slice(0, 2)) * 60 + Number(interval.close.slice(3)))
    if (active) return `Open now · closes ${timeLabel(active.close)}`
    const nextToday = todayHours.find((interval) => minutesNow < Number(interval.open.slice(0, 2)) * 60 + Number(interval.open.slice(3)))
    if (nextToday) return `Closed now · opens today ${timeLabel(nextToday.open)}`
    const todayIndex = weekdays.indexOf(today)
    for (let dayOffset = 1; dayOffset < weekdays.length; dayOffset += 1) {
      const nextDay = weekdays[(todayIndex + dayOffset) % weekdays.length]
      const firstInterval = branch.openingHours[nextDay][0]
      if (firstInterval) {
        const label = nextDay[0].toUpperCase() + nextDay.slice(1)
        return `Closed now · opens ${label} ${timeLabel(firstInterval.open)}`
      }
    }
  } catch {
    return 'Opening hours use branch local time'
  }
  return 'Closed now'
}

export function StationCatalogPanel({ apiBaseUrl, accessToken, tenantId, branchId, stationId, branchName, stationName, canCheckOtherBranches }: {
  apiBaseUrl: string
  accessToken: string
  tenantId: string
  branchId: string
  stationId: string
  branchName: string
  stationName: string
  canCheckOtherBranches: boolean
}) {
  const [catalog, setCatalog] = useState<LoadState>({ kind: 'loading' })
  const [query, setQuery] = useState('')
  const [selectedProductId, setSelectedProductId] = useState<string | null>(null)
  const [availability, setAvailability] = useState<AvailabilityState | null>(null)
  const [nowMs, setNowMs] = useState(() => Date.now())
  const availabilityRequest = useRef(0)

  useEffect(() => {
    const timer = window.setInterval(() => setNowMs(Date.now()), 15_000)
    return () => window.clearInterval(timer)
  }, [])

  const load = useCallback(async () => {
    availabilityRequest.current += 1
    setSelectedProductId(null)
    setAvailability(null)
    setCatalog({ kind: 'loading' })
    try {
      const products = await listStationProducts({ apiBaseUrl, accessToken, tenantId, branchId, stationId })
      setCatalog({ kind: 'ready', products })
    } catch (reason) {
      setCatalog({ kind: 'error', message: messageOf(reason, 'The station catalog could not be loaded.') })
    }
  }, [accessToken, apiBaseUrl, branchId, stationId, tenantId])

  useEffect(() => { void load() }, [load])

  const visibleProducts = useMemo(() => catalog.kind === 'ready'
    ? catalog.products.filter((product) => `${product.name} ${product.sku} ${product.barcode ?? ''} ${product.category ?? ''}`.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()))
    : [], [catalog, query])

  function acceptScan(value: string) {
    const normalized = value.trim().toLocaleLowerCase()
    if (!normalized || catalog.kind !== 'ready') return
    const exact = catalog.products.find((product) => product.barcode?.toLocaleLowerCase() === normalized || product.sku.toLocaleLowerCase() === normalized)
    if (exact) {
      setQuery(exact.sku)
      setSelectedProductId(exact.productId)
      setAvailability(null)
    }
  }

  function changeQuery(value: string) {
    availabilityRequest.current += 1
    setQuery(value)
    setSelectedProductId(null)
    setAvailability(null)
  }

  async function checkAvailability(product: BranchProduct) {
    const requestId = ++availabilityRequest.current
    setSelectedProductId(product.productId)
    setAvailability({ kind: 'loading' })
    try {
      const branches = await getProductBranchAvailability({ apiBaseUrl, accessToken, tenantId, branchId, stationId, productId: product.productId })
      if (availabilityRequest.current === requestId) setAvailability({ kind: 'ready', branches })
    } catch (reason) {
      if (availabilityRequest.current === requestId) setAvailability({ kind: 'error', message: messageOf(reason, 'Branch availability could not be loaded.') })
    }
  }

  const selectedProduct = catalog.kind === 'ready' ? catalog.products.find((product) => product.productId === selectedProductId) : undefined

  return <section className="station-catalog" aria-labelledby="station-catalog-title">
    <header className="station-catalog-header">
      <div className="catalog-title-group"><span className="catalog-icon"><Boxes size={18}/></span><div><p className="eyebrow">REGISTER CATALOG</p><h2 id="station-catalog-title">Find a product</h2><p>Search or scan at {stationName}. Check other branches when an item is unavailable here.</p></div></div>
      <button type="button" className="button button-quiet catalog-refresh" onClick={() => void load()} disabled={catalog.kind === 'loading'}><RefreshCw size={15}/> Refresh</button>
    </header>

    <label className="catalog-search station-product-search"><Search size={16}/><input type="search" value={query} onChange={(event) => changeQuery(event.target.value)} onKeyDown={(event) => { if (event.key === 'Enter') acceptScan(event.currentTarget.value) }} placeholder="Scan barcode or search name, SKU, category" aria-label="Search or scan register products"/><Barcode size={16}/></label>
    <p className="station-scan-hint">Barcode scanners that type into the search field can scan and press Enter.</p>

    {catalog.kind === 'loading' && <div className="catalog-state"><span className="mini-spinner"/><span>Loading station products…</span></div>}
    {catalog.kind === 'error' && <div className="catalog-state catalog-state-error" role="alert"><CircleAlert size={18}/><span>{catalog.message}</span><button type="button" className="button button-quiet" onClick={() => void load()}>Try again</button></div>}
    {catalog.kind === 'ready' && catalog.products.length === 0 && <div className="catalog-state catalog-empty"><strong>No products are assigned to this station</strong><span>A manager can add products to the branch catalog.</span></div>}
    {catalog.kind === 'ready' && catalog.products.length > 0 && visibleProducts.length === 0 && <div className="catalog-state">No products match “{query}”.</div>}

    {catalog.kind === 'ready' && visibleProducts.length > 0 && <div className="station-product-list" aria-label="Register products">{visibleProducts.map((product) => {
      const selected = selectedProductId === product.productId
      return <article className={`station-product-card${selected ? ' selected' : ''}`} key={product.productId}>
        <div className="station-product-main"><strong>{product.name}</strong><small><span>{product.sku}</span>{product.barcode && <><span aria-hidden="true">·</span><span>{product.barcode}</span></>}</small></div>
        <div className="station-product-stock"><strong className={product.stockQuantity === 0 ? 'stock-zero' : product.stockQuantity <= product.reorderPoint ? 'stock-low' : ''}>{product.stockQuantity}</strong><small>in {branchName}</small></div>
        <div className="station-product-price"><strong>{formatMoney(product.currencyCode, product.priceMinor)}</strong><small>selling price</small></div>
        {canCheckOtherBranches && <button type="button" className="button button-quiet station-availability-button" onClick={() => void checkAvailability(product)} aria-expanded={selected && availability !== null} aria-label={`Check ${product.name} in other branches`}><Store size={14}/><span>Other branches</span></button>}
        {selected && availability && <div className="station-availability" aria-live="polite">
          {availability.kind === 'loading' && <span><span className="mini-spinner"/> Checking branch stock…</span>}
          {availability.kind === 'error' && <span className="station-availability-error" role="alert"><CircleAlert size={15}/>{availability.message}</span>}
          {availability.kind === 'ready' && <>
            <div className="station-availability-heading"><strong>Availability across your business</strong><span className="station-availability-actions"><button type="button" onClick={() => selectedProduct && void checkAvailability(selectedProduct)} aria-label="Refresh branch availability" title="Refresh branch availability"><RefreshCw size={15}/></button><button type="button" onClick={() => setAvailability(null)} aria-label="Close branch availability">×</button></span></div>
            {availability.branches[0] && <p className="station-availability-checked">{isAvailabilityStale(availability.branches[0].observedAt, nowMs) ? 'Stale snapshot' : 'Server snapshot'} · checked {formatObservedAt(availability.branches[0].observedAt)}{isAvailabilityStale(availability.branches[0].observedAt, nowMs) ? ' · refresh before directing a customer' : ''}</p>}
            {availability.branches.map((branch) => {
              const stale = isAvailabilityStale(branch.observedAt, nowMs)
              const committed = branch.pickupReservedQuantity + branch.outboundCommittedQuantity
              return <div className={`station-branch-stock${stale ? ' stale' : ''}`} key={branch.branchId}>
              <span className="station-branch-name"><strong>{branch.branchName}</strong><small>{branch.branchCode}{branch.branchId === branchId ? ' · This branch' : ''}</small></span>
              <div className="station-branch-quantity"><strong className={branch.availableToPromise === 0 ? 'stock-zero' : ''}>{stale ? `${branch.availableToPromise} last seen` : `${branch.availableToPromise} available`}</strong><small>{branch.stockQuantity} on hand{committed > 0 ? ` · ${committed} committed` : ''}</small></div>
              <span>{formatMoney(branch.currencyCode, branch.priceMinor)}</span>
              <div className="station-branch-details">
                {branch.commitmentShortfall > 0 && <span className="station-branch-shortfall">Commitments exceed stock by {branch.commitmentShortfall}; contact the branch before promising an item.</span>}
                {branch.address && <span><MapPin size={13}/>{branch.address}</span>}
                {branch.phone && <a href={`tel:${branch.phone.replace(/[^\d+]/g, '')}`}><Phone size={13}/>{branch.phone}</a>}
                {branch.email && <a href={`mailto:${branch.email}`}><Mail size={13}/>{branch.email}</a>}
                <span className="station-branch-hours"><Clock3 size={13}/>{branchOpeningStatus(branch)} · branch local time</span>
              </div>
            </div>})}
            {availability.branches.length === 0 && <p>No active branches have this product assigned.</p>}
          </>}
        </div>}
      </article>
    })}</div>}

    {selectedProduct && availability?.kind === 'ready' && <p className="station-catalog-disclosure">Branch results are snapshots and do not reserve inventory or move stock. Refresh before directing a customer; stale results are labeled and should not be treated as current stock.</p>}
    <footer className="station-catalog-footer"><span>Viewing {catalog.kind === 'ready' ? visibleProducts.length : 0} products</span><span>Branch checks are snapshots; refresh before relying on stock. They do not reserve or transfer stock.</span></footer>
  </section>
}
