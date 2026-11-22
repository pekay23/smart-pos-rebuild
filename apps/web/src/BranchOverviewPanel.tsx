// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useCallback, useEffect, useRef, useState } from 'react'
import { Activity, Package, RefreshCw, TrendingUp } from 'lucide-react'
import { getHostedBranchSalesReport, listBranchProducts } from './api'
import type { BranchProduct, HostedSalesReport } from './api'

function dateInZone(timeZone: string, daysAgo: number): string {
  const parts = new Intl.DateTimeFormat('en-US', {
    timeZone,
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
  }).formatToParts(new Date())
  const part = (type: Intl.DateTimeFormatPartTypes) => parts.find((entry) => entry.type === type)?.value ?? ''
  const date = new Date(Date.UTC(Number(part('year')), Number(part('month')) - 1, Number(part('day'))))
  date.setUTCDate(date.getUTCDate() - daysAgo)
  return date.toISOString().slice(0, 10)
}

function nextDate(dateText: string): string {
  const date = new Date(`${dateText}T00:00:00.000Z`)
  date.setUTCDate(date.getUTCDate() + 1)
  return date.toISOString().slice(0, 10)
}

function formatMoney(currency: string, amountMinor: string): string {
  const amount = BigInt(amountMinor)
  return `${currency} ${(amount / 100n).toLocaleString()}.${String(amount % 100n).padStart(2, '0')}`
}

type Snapshot = { sales: HostedSalesReport | null; lowStockCount: number; lowStockProducts: BranchProduct[] }

export function BranchOverviewPanel({
  apiBaseUrl, accessToken, tenantId, branchId, branchName, timeZone, canViewSales, canViewInventory,
}: {
  apiBaseUrl: string
  accessToken: string
  tenantId: string
  branchId: string
  branchName: string
  timeZone: string
  canViewSales: boolean
  canViewInventory: boolean
}) {
  const [snapshot, setSnapshot] = useState<Snapshot>({ sales: null, lowStockCount: 0, lowStockProducts: [] })
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const requestId = useRef(0)
  const refresh = useCallback(async () => {
    const currentRequest = ++requestId.current
    setLoading(true)
    setError(null)
    setSnapshot({ sales: null, lowStockCount: 0, lowStockProducts: [] })
    try {
      const fromDate = dateInZone(timeZone, 6)
      const toDate = nextDate(dateInZone(timeZone, 0))
      const [sales, products] = await Promise.all([
        canViewSales ? getHostedBranchSalesReport({
          apiBaseUrl, accessToken, tenantId, branchId, fromDate, toDate, offset: 0, pageSize: 1,
        }) : Promise.resolve(null),
        canViewInventory ? listBranchProducts({ apiBaseUrl, accessToken, tenantId, branchId }) : Promise.resolve([]),
      ])
      if (requestId.current === currentRequest) {
        const lowStock = products
          .filter((product) => product.isActive && product.stockQuantity <= product.reorderPoint)
          .sort((left, right) => left.stockQuantity - right.stockQuantity || left.name.localeCompare(right.name))
        setSnapshot({ sales, lowStockCount: lowStock.length, lowStockProducts: lowStock.slice(0, 6) })
      }
    } catch (reason) {
      if (requestId.current === currentRequest) setError(reason instanceof Error ? reason.message : 'The branch overview could not be loaded.')
    } finally {
      if (requestId.current === currentRequest) setLoading(false)
    }
  }, [accessToken, apiBaseUrl, branchId, canViewInventory, canViewSales, tenantId, timeZone])

  useEffect(() => { void refresh() }, [refresh])

  const totals = snapshot.sales?.totalsByCurrency ?? []
  return <section className="branch-overview" aria-labelledby="branch-overview-title">
    <header className="branch-overview-header">
      <div><p className="eyebrow">BRANCH SNAPSHOT</p><h2 id="branch-overview-title">{branchName} at a glance</h2><p>Last 7 business days · {timeZone}</p></div>
      <button className="button button-quiet" type="button" onClick={() => void refresh()} disabled={loading} aria-label="Refresh branch overview"><RefreshCw size={15}/><span>Refresh</span></button>
    </header>
    {error && <div className="notice notice-danger" role="alert">{error}</div>}
    <div className={`branch-overview-grid ${canViewSales && canViewInventory ? '' : 'branch-overview-grid-single'}`} aria-live="polite">
      {canViewSales && <article className="overview-card">
        <div className="overview-card-title"><span className="overview-icon"><TrendingUp size={17}/></span><span>Sales · 7 days</span></div>
        {loading && !snapshot.sales ? <p className="overview-loading" role="status">Loading sales…</p> : <>
          <strong className="overview-card-value">{snapshot.sales?.matchedCount.toLocaleString() ?? '—'}</strong>
          <span className="overview-card-caption">completed sales</span>
          <div className="overview-currency-totals">{totals.length > 0 ? totals.map((item) => <div key={item.currencyCode}><span>{item.currencyCode}</span><strong>{formatMoney(item.currencyCode, item.totalMinor)}</strong></div>) : <span>{snapshot.sales ? 'No sales recorded' : 'Sales summary unavailable'}</span>}</div>
        </>}
      </article>}
      {canViewInventory && <article className="overview-card overview-stock-card">
        <div className="overview-card-title"><span className="overview-icon overview-icon-stock"><Package size={17}/></span><span>Reorder attention</span></div>
        {loading && snapshot.lowStockProducts.length === 0 ? <p className="overview-loading" role="status">Checking stock…</p> : <>
          <strong className="overview-card-value">{snapshot.lowStockCount}</strong>
          <span className="overview-card-caption">products at or below reorder point</span>
          <div className="overview-low-stock-list">{snapshot.lowStockProducts.length > 0 ? snapshot.lowStockProducts.map((product) => <div key={product.productId}><span><strong>{product.name}</strong><small>{product.sku}</small></span><span className="stock-level">{product.stockQuantity} <small>left · reorder at {product.reorderPoint}</small></span></div>) : <span>{loading ? 'Loading stock levels…' : 'No products need reordering'}</span>}</div>
        </>}
      </article>}
      {!canViewSales && !canViewInventory && <div className="empty-inline"><Activity size={17}/>No sales or inventory overview permission is assigned for this branch.</div>}
    </div>
    <p className="branch-overview-disclosure">Sales totals use recorded checkout data and do not confirm processor settlement. Stock alerts compare current branch stock with each product’s configured reorder point.</p>
  </section>
}
