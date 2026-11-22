// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useEffect, useState } from 'react'
import { AlertTriangle, ArrowRight, Building2, Clock3, RefreshCw, TrendingUp } from 'lucide-react'
import { getTenantOverview } from './api'
import type { TenantOverview } from './api'

function formatMoney(currency: string, amountMinor: string): string {
  const amount = BigInt(amountMinor)
  return `${currency} ${(amount / 100n).toLocaleString()}.${String(amount % 100n).padStart(2, '0')}`
}

export function TenantOverviewPanel({
  apiBaseUrl, accessToken, tenantId, tenantName, onOpenBranch,
}: {
  apiBaseUrl: string
  accessToken: string
  tenantId: string
  tenantName: string
  onOpenBranch: (branchId: string) => void
}) {
  const [offset, setOffset] = useState(0)
  const [overview, setOverview] = useState<TenantOverview | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [refreshKey, setRefreshKey] = useState(0)

  useEffect(() => {
    let active = true
    setLoading(true)
    setError(null)
    setOverview(null)
    void getTenantOverview({ apiBaseUrl, accessToken, tenantId, offset, pageSize: 12 })
      .then((data) => { if (active) setOverview(data) })
      .catch((reason: unknown) => { if (active) setError(reason instanceof Error ? reason.message : 'The business overview could not be loaded.') })
      .finally(() => { if (active) setLoading(false) })
    return () => { active = false }
  }, [accessToken, apiBaseUrl, offset, refreshKey, tenantId])

  const pageEnd = Math.min(offset + (overview?.branches.length ?? 0), overview?.totalBranchCount ?? 0)
  return <section className="tenant-overview" aria-labelledby="tenant-overview-title">
    <header className="tenant-overview-header">
      <div><p className="eyebrow">BUSINESS OVERVIEW</p><h2 id="tenant-overview-title">{tenantName} · all assigned branches</h2><p>Sales for the last 7 × 24 hours · stock alerts by branch</p></div>
      <button className="button button-quiet" type="button" onClick={() => setRefreshKey((value) => value + 1)} disabled={loading} aria-label="Refresh business overview"><RefreshCw size={15}/><span>Refresh</span></button>
    </header>
    {error && <div className="notice notice-danger" role="alert">{error}</div>}
    {loading && !overview && <p className="overview-loading" role="status">Loading assigned branches…</p>}
    {overview && <>
      <div className="tenant-overview-grid" aria-live="polite">
        {overview.branches.map((branch) => <article className="tenant-overview-branch" key={branch.branchId}>
          <header><span className="tenant-overview-branch-icon"><Building2 size={17}/></span><span><strong>{branch.branchName}</strong><small>{branch.timezone} · {branch.currencyCode}</small></span></header>
          <div className="tenant-overview-metrics">
            {branch.sales && <div className="tenant-overview-metric"><span><TrendingUp size={14}/> Sales · 7 days</span><strong>{branch.sales.saleCount.toLocaleString()}</strong><small>completed sales</small>{branch.sales.totalsByCurrency.map((total) => <div className="tenant-overview-total" key={total.currencyCode}><span>{total.currencyCode}</span><strong>{formatMoney(total.currencyCode, total.totalMinor)}</strong></div>)}</div>}
            {branch.inventory && <div className="tenant-overview-metric"><span><AlertTriangle size={14}/> Reorder attention</span><strong>{branch.inventory.count.toLocaleString()}</strong><small>products at or below reorder point</small>{branch.inventory.products.length > 0 && <ul>{branch.inventory.products.slice(0, 3).map((product) => <li key={product.productId}><span>{product.name}</span><strong>{product.stockQuantity} left</strong></li>)}</ul>}</div>}
            {branch.attendance && <div className="tenant-overview-metric tenant-overview-attendance"><span><Clock3 size={14}/> Attendance today</span><strong>{branch.attendance.activeStaffCount.toLocaleString()} on shift</strong><small>{branch.attendance.startedTodayCount.toLocaleString()} staff started today</small></div>}
          </div>
          <button type="button" className="tenant-overview-open" onClick={() => onOpenBranch(branch.branchId)}>Open branch workspace <ArrowRight size={14}/></button>
        </article>)}
      </div>
      <footer className="tenant-overview-footer"><span>{overview.totalBranchCount.toLocaleString()} branch{overview.totalBranchCount === 1 ? '' : 'es'} in your authorized overview</span><div><button className="button button-quiet" type="button" onClick={() => setOffset(Math.max(0, offset - overview.pageSize))} disabled={loading || offset === 0}>Previous</button><button className="button button-quiet" type="button" onClick={() => setOffset(offset + overview.pageSize)} disabled={loading || pageEnd >= overview.totalBranchCount}>Next</button></div></footer>
    </>}
    <p className="tenant-overview-disclosure">The API includes only branches where your account has sales or product read access. Sales are recorded manual tenders and do not confirm processor settlement. Branches may use different currencies, so totals remain separate.</p>
  </section>
}
